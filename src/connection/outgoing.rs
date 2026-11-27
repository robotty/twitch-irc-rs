use crate::connection::event_loop::ConnectionLoopCommand;
use crate::error::Error;
use crate::irc;
use crate::login::LoginCredentials;
use crate::message::{AsRawIRC, IRCMessage};
#[cfg(feature = "metrics-collection")]
use crate::metrics::MetricsBundle;
use crate::rate_limit::RateLimiter;
use crate::transport::Transport;
use futures_util::SinkExt;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Weak};
use tokio::sync::{mpsc, oneshot};

pub(super) type PriorityMessageReceiver<T, L> =
    mpsc::UnboundedReceiver<(IRCMessage, Option<oneshot::Sender<Result<(), Error<T, L>>>>)>;
pub(super) type PriorityMessageSender<T, L> =
    mpsc::UnboundedSender<(IRCMessage, Option<oneshot::Sender<Result<(), Error<T, L>>>>)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ChannelOpKind {
    Join,
    Part,
}

#[derive(Debug)]
pub(super) enum ChannelOp {
    Join(String),
    Part(String),
}

pub(super) enum OutgoingMessage {
    ChannelOp(ChannelOp),
    Priority(IRCMessage),
}

pub(super) fn classify_outgoing_message<T: Transport, L: LoginCredentials>(
    message: IRCMessage,
    reply_sender: &Option<oneshot::Sender<Result<(), Error<T, L>>>>,
) -> OutgoingMessage {
    if reply_sender.is_none()
        && message.tags.0.is_empty()
        && message.prefix.is_none()
        && message.params.len() == 1
    {
        match message.command.as_str() {
            "JOIN" => {
                let target = message.params.into_iter().next().unwrap();
                return OutgoingMessage::ChannelOp(ChannelOp::Join(target));
            }
            "PART" => {
                let target = message.params.into_iter().next().unwrap();
                return OutgoingMessage::ChannelOp(ChannelOp::Part(target));
            }
            _ => {}
        }
    }
    OutgoingMessage::Priority(message)
}

#[derive(Default)]
struct PendingChannelOps {
    state: HashMap<String, ChannelOpKind>,
    joins: VecDeque<String>,
    parts: VecDeque<String>,
}

impl PendingChannelOps {
    fn enqueue(&mut self, op: ChannelOp) {
        let (target, kind, queue) = match op {
            ChannelOp::Join(ch) => (ch, ChannelOpKind::Join, &mut self.joins),
            ChannelOp::Part(ch) => (ch, ChannelOpKind::Part, &mut self.parts),
        };
        if self.state.get(&target).copied() == Some(kind) {
            return;
        }
        self.state.insert(target.clone(), kind);
        queue.push_back(target);
    }

    fn trim_stale(
        state: &HashMap<String, ChannelOpKind>,
        queue: &mut VecDeque<String>,
        expected: ChannelOpKind,
    ) {
        while let Some(front) = queue.front() {
            if state.get(front).copied() == Some(expected) {
                break;
            }
            queue.pop_front();
        }
    }

    fn has_join(&mut self) -> bool {
        Self::trim_stale(&self.state, &mut self.joins, ChannelOpKind::Join);
        !self.joins.is_empty()
    }

    fn has_part(&mut self) -> bool {
        Self::trim_stale(&self.state, &mut self.parts, ChannelOpKind::Part);
        !self.parts.is_empty()
    }

    fn pop_join(&mut self) -> Option<String> {
        Self::trim_stale(&self.state, &mut self.joins, ChannelOpKind::Join);
        let ch = self.joins.pop_front()?;
        self.state.remove(&ch);
        Some(ch)
    }

    fn pop_part(&mut self) -> Option<String> {
        Self::trim_stale(&self.state, &mut self.parts, ChannelOpKind::Part);
        let ch = self.parts.pop_front()?;
        self.state.remove(&ch);
        Some(ch)
    }
}

async fn send_one<T: Transport, L: LoginCredentials>(
    transport_outgoing: &mut T::Outgoing,
    connection_loop_tx: &Weak<mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>>,
    message: IRCMessage,
    reply_sender: Option<oneshot::Sender<Result<(), Error<T, L>>>>,
    #[cfg(feature = "metrics-collection")] metrics: Option<&MetricsBundle>,
) -> bool {
    tracing::trace!("> {}", message.as_raw_irc());
    #[cfg(feature = "metrics-collection")]
    if let Some(metrics) = metrics {
        metrics
            .messages_sent
            .with_label_values(&[&message.command])
            .inc();
    }

    let res = transport_outgoing.send(message).await.map_err(Arc::new);

    // The error is cloned and sent both to the calling method as well as
    // the connection event loop so it can end with that error.
    if let Err(ref err) = res
        && let Some(connection_loop_tx) = connection_loop_tx.upgrade()
    {
        connection_loop_tx
            .send(ConnectionLoopCommand::SendError(Arc::clone(err)))
            .ok();
    }

    let ok = res.is_ok();
    if let Some(reply_sender) = reply_sender {
        reply_sender.send(res.map_err(Error::OutgoingError)).ok();
    }
    ok
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_outgoing_forward_task<T: Transport, L: LoginCredentials>(
    mut transport_outgoing: T::Outgoing,
    mut priority_rx: PriorityMessageReceiver<T, L>,
    mut channel_op_rx: mpsc::UnboundedReceiver<ChannelOp>,
    mut shutdown_notify: oneshot::Receiver<()>,
    connection_loop_tx: Weak<mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>>,
    join_rate_limiter: Arc<RateLimiter>,
    part_rate_limiter: Arc<RateLimiter>,
    #[cfg(feature = "metrics-collection")] metrics: Option<MetricsBundle>,
) {
    tracing::debug!("Spawned outgoing messages forwarder");

    let mut pending_ops = PendingChannelOps::default();
    let mut channel_ops_closed = false;

    loop {
        while let Ok((message, reply_sender)) = priority_rx.try_recv() {
            if !send_one(
                &mut transport_outgoing,
                &connection_loop_tx,
                message,
                reply_sender,
                #[cfg(feature = "metrics-collection")]
                metrics.as_ref(),
            )
            .await
            {
                return;
            }
        }

        while !channel_ops_closed {
            match channel_op_rx.try_recv() {
                Ok(op) => pending_ops.enqueue(op),
                Err(mpsc::error::TryRecvError::Empty) => break,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    channel_ops_closed = true;
                }
            }
        }

        let has_part = pending_ops.has_part();
        let has_join = pending_ops.has_join();

        tokio::select! {
            biased;
            _ = &mut shutdown_notify => {
                break;
            }
            maybe_prio = priority_rx.recv() => {
                match maybe_prio {
                    Some((message, reply_sender)) => {
                        if !send_one(
                            &mut transport_outgoing,
                            &connection_loop_tx,
                            message,
                            reply_sender,
                            #[cfg(feature = "metrics-collection")]
                            metrics.as_ref(),
                        )
                        .await
                        {
                            return;
                        }
                    }
                    None => break,
                }
            }
            maybe_op = channel_op_rx.recv(), if !channel_ops_closed => {
                match maybe_op {
                    Some(op) => pending_ops.enqueue(op),
                    None => channel_ops_closed = true,
                }
            }
            () = part_rate_limiter.acquire(), if has_part => {
                if let Some(channel) = pending_ops.pop_part()
                    && !send_one(
                        &mut transport_outgoing,
                        &connection_loop_tx,
                        irc!["PART", channel],
                        None,
                        #[cfg(feature = "metrics-collection")]
                        metrics.as_ref(),
                    )
                    .await
                {
                    return;
                }
            }
            () = join_rate_limiter.acquire(), if has_join => {
                if let Some(channel) = pending_ops.pop_join()
                    && !send_one(
                        &mut transport_outgoing,
                        &connection_loop_tx,
                        irc!["JOIN", channel],
                        None,
                        #[cfg(feature = "metrics-collection")]
                        metrics.as_ref(),
                    )
                    .await
                {
                    return;
                }
            }
        }
    }
    tracing::debug!("Outgoing messages forwarder ended");
}
