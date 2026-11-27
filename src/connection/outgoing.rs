use crate::error::Error;
use crate::irc;
use crate::login::LoginCredentials;
use crate::message::{AsRawIRC, IRCMessage};
#[cfg(feature = "metrics-collection")]
use crate::metrics::MetricsBundle;
use crate::rate_limit::RateLimiter;
use crate::transport::Transport;
use futures_util::sink::Sink;
use std::collections::{HashMap, VecDeque};
use std::future::poll_fn;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::oneshot;

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

type OutgoingReplySender<T, L> = oneshot::Sender<Result<(), Error<T, L>>>;

/// Cancel-safe outgoing sink driver that multiplexes priority IRC commands, global rate-limited
/// `JOIN`/`PART` channel operations (with coalescing), and async sink flushing from a single task.
pub(super) struct OutgoingSink<T: Transport, L: LoginCredentials> {
    transport: T::Outgoing,
    priority_queue: VecDeque<(IRCMessage, Option<OutgoingReplySender<T, L>>)>,
    pending_ops: PendingChannelOps,
    flushing_replies: VecDeque<OutgoingReplySender<T, L>>,
    needs_flush: bool,
    #[cfg(feature = "metrics-collection")]
    metrics: Option<MetricsBundle>,
}

impl<T: Transport, L: LoginCredentials> OutgoingSink<T, L> {
    pub(super) fn new(
        transport: T::Outgoing,
        #[cfg(feature = "metrics-collection")] metrics: Option<MetricsBundle>,
    ) -> Self {
        OutgoingSink {
            transport,
            priority_queue: VecDeque::new(),
            pending_ops: PendingChannelOps::default(),
            flushing_replies: VecDeque::new(),
            needs_flush: false,
            #[cfg(feature = "metrics-collection")]
            metrics,
        }
    }

    pub(super) fn enqueue_message(
        &mut self,
        message: IRCMessage,
        reply_sender: Option<OutgoingReplySender<T, L>>,
    ) {
        match classify_outgoing_message(message, &reply_sender) {
            OutgoingMessage::ChannelOp(op) => self.pending_ops.enqueue(op),
            OutgoingMessage::Priority(message) => {
                self.priority_queue.push_back((message, reply_sender));
            }
        }
    }

    pub(super) fn has_work(&mut self) -> bool {
        !self.priority_queue.is_empty()
            || self.needs_flush
            || self.pending_ops.has_part()
            || self.pending_ops.has_join()
    }

    fn start_send_message(
        &mut self,
        message: IRCMessage,
        reply_sender: Option<OutgoingReplySender<T, L>>,
    ) -> Result<(), Arc<T::OutgoingError>> {
        tracing::trace!("> {}", message.as_raw_irc());
        #[cfg(feature = "metrics-collection")]
        if let Some(ref metrics) = self.metrics {
            metrics
                .messages_sent
                .with_label_values(&[&message.command])
                .inc();
        }

        match Pin::new(&mut self.transport).start_send(message) {
            Ok(()) => {
                self.needs_flush = true;
                if let Some(reply_sender) = reply_sender {
                    self.flushing_replies.push_back(reply_sender);
                }
                Ok(())
            }
            Err(err) => {
                let err = Arc::new(err);
                if let Some(reply_sender) = reply_sender {
                    reply_sender
                        .send(Err(Error::OutgoingError(Arc::clone(&err))))
                        .ok();
                }
                Err(err)
            }
        }
    }

    /// Drives one outgoing step (sending a priority message, flushing buffered frames, or acquiring
    /// a `PART`/`JOIN` rate-limiter permit and submitting one channel op).
    ///
    /// Cancel-safe: internal queues and rate-limiter permits are only mutated synchronously once the
    /// awaited readiness/flush/limiter futures resolve.
    pub(super) async fn drive_once(
        &mut self,
        join_rate_limiter: &RateLimiter,
        part_rate_limiter: &RateLimiter,
    ) -> Result<(), Arc<T::OutgoingError>> {
        if !self.priority_queue.is_empty() {
            poll_fn(|cx| Pin::new(&mut self.transport).poll_ready(cx))
                .await
                .map_err(Arc::new)?;
            if let Some((message, reply_sender)) = self.priority_queue.pop_front() {
                self.start_send_message(message, reply_sender)?;
            }
            return Ok(());
        }

        if self.needs_flush {
            poll_fn(|cx| Pin::new(&mut self.transport).poll_flush(cx))
                .await
                .map_err(Arc::new)?;
            self.needs_flush = false;
            for reply_sender in self.flushing_replies.drain(..) {
                reply_sender.send(Ok(())).ok();
            }
            return Ok(());
        }

        let has_part = self.pending_ops.has_part();
        let has_join = self.pending_ops.has_join();
        if !has_part && !has_join {
            return Ok(());
        }

        poll_fn(|cx| Pin::new(&mut self.transport).poll_ready(cx))
            .await
            .map_err(Arc::new)?;

        tokio::select! {
            biased;
            () = part_rate_limiter.acquire(), if has_part => {
                if let Some(channel) = self.pending_ops.pop_part() {
                    self.start_send_message(irc!["PART", channel], None)?;
                }
            }
            () = join_rate_limiter.acquire(), if has_join => {
                if let Some(channel) = self.pending_ops.pop_join() {
                    self.start_send_message(irc!["JOIN", channel], None)?;
                }
            }
        }

        Ok(())
    }

    pub(super) fn fail_all_pending(&mut self, cause: &Error<T, L>) {
        for reply_sender in self.flushing_replies.drain(..) {
            reply_sender.send(Err(cause.clone())).ok();
        }
        for (_message, reply_sender) in self.priority_queue.drain(..) {
            if let Some(reply_sender) = reply_sender {
                reply_sender.send(Err(cause.clone())).ok();
            }
        }
    }
}
