use super::outgoing::{ChannelOp, OutgoingMessage, OutgoingSink, classify_outgoing_message};
use crate::client::event_loop::ClientLoopCommand;
use crate::config::ClientConfig;
use crate::connection::ConnectionIncomingMessage;
use crate::error::Error;
use crate::irc;
use crate::login::{CredentialsPair, LoginCredentials};
use crate::message::AsRawIRC;
use crate::message::IRCMessage;
use crate::message::commands::ServerMessage;
#[cfg(feature = "metrics-collection")]
use crate::metrics::MetricsBundle;
use crate::transport::Transport;
use either::Either;
use futures_util::StreamExt;
use std::collections::{HashSet, VecDeque};
use std::convert::TryFrom;
use std::sync::{Arc, Weak};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Duration, Instant, interval_at};
use tracing::{Instrument, debug_span, info_span};

#[derive(Debug)]
pub(crate) enum ConnectionLoopCommand<T: Transport, L: LoginCredentials> {
    // commands that come from Connection methods
    SendMessage(IRCMessage, Option<oneshot::Sender<Result<(), Error<T, L>>>>),
}

fn send_to_client<T: Transport, L: LoginCredentials>(
    client_loop_tx: &Weak<mpsc::UnboundedSender<ClientLoopCommand<T, L>>>,
    connection_id: usize,
    message: ConnectionIncomingMessage<T, L>,
) {
    if let Some(client_loop_tx) = client_loop_tx.upgrade() {
        client_loop_tx
            .send(ClientLoopCommand::IncomingMessage {
                source_connection_id: connection_id,
                message: Box::new(message),
            })
            .ok();
    }
}

pub(crate) struct ConnectionLoopWorker;

impl ConnectionLoopWorker {
    pub fn spawn<T: Transport, L: LoginCredentials>(
        config: Arc<ClientConfig<L>>,
        client_loop_tx: Weak<mpsc::UnboundedSender<ClientLoopCommand<T, L>>>,
        connection_loop_rx: mpsc::UnboundedReceiver<ConnectionLoopCommand<T, L>>,
        connection_id: usize,
        #[cfg(feature = "metrics-collection")] metrics: Option<MetricsBundle>,
    ) {
        let span = info_span!("connection", id = connection_id);
        tokio::spawn(
            Self::run(
                config,
                client_loop_tx,
                connection_loop_rx,
                connection_id,
                #[cfg(feature = "metrics-collection")]
                metrics,
            )
            .instrument(span),
        );
    }

    async fn run<T: Transport, L: LoginCredentials>(
        config: Arc<ClientConfig<L>>,
        client_loop_tx: Weak<mpsc::UnboundedSender<ClientLoopCommand<T, L>>>,
        mut connection_loop_rx: mpsc::UnboundedReceiver<ConnectionLoopCommand<T, L>>,
        connection_id: usize,
        #[cfg(feature = "metrics-collection")] metrics: Option<MetricsBundle>,
    ) {
        tracing::debug!("Spawned connection event loop");

        // Phase 1: Initializing
        let mut commands_queue = VecDeque::new();
        let mut queued_joins: HashSet<String> = HashSet::new();
        let mut duplicate_joins_dropped = 0usize;
        let mut joins_cancelled_by_part = 0usize;

        let init_fut = Self::init_transport::<T, L>(Arc::clone(&config));
        tokio::pin!(init_fut);

        let init_result = loop {
            tokio::select! {
                init_result = &mut init_fut => break init_result,
                maybe_cmd = connection_loop_rx.recv() => {
                    match maybe_cmd {
                        Some(ConnectionLoopCommand::SendMessage(message, reply_sender)) => {
                            match classify_outgoing_message(message, &reply_sender) {
                                OutgoingMessage::ChannelOp(ChannelOp::Join(target)) => {
                                    if !queued_joins.insert(target) {
                                        duplicate_joins_dropped += 1;
                                    }
                                }
                                OutgoingMessage::ChannelOp(ChannelOp::Part(target)) => {
                                    if queued_joins.remove(&target) {
                                        joins_cancelled_by_part += 1;
                                    }
                                }
                                OutgoingMessage::Priority(message) => {
                                    commands_queue.push_back((message, reply_sender));
                                }
                            }
                        }
                        None => {
                            tracing::debug!("Connection event loop ended during initialization");
                            return;
                        }
                    }
                }
            }
        };

        let (transport, credentials) = match init_result {
            Ok(pair) => pair,
            Err(init_error) => {
                tracing::error!("Transport init task has finished with error, closing connection");
                tracing::info!("Closing connection, reason: {}", init_error);

                for (_message, return_sender) in commands_queue {
                    if let Some(return_sender) = return_sender {
                        return_sender.send(Err(init_error.clone())).ok();
                    }
                }

                send_to_client(
                    &client_loop_tx,
                    connection_id,
                    ConnectionIncomingMessage::StateClosed {
                        cause: init_error.clone(),
                    },
                );

                Self::drain_closed(&mut connection_loop_rx, init_error).await;
                tracing::debug!("Connection event loop ended");
                return;
            }
        };

        // Phase 2: Open
        tracing::debug!("Transport init task has finished, transitioning to Open");
        let (mut transport_incoming, transport_outgoing) = transport.split();

        #[cfg(feature = "metrics-collection")]
        {
            if let Some(ref metrics) = metrics {
                metrics
                    .connection_init_queued_joins
                    .observe(queued_joins.len() as f64);
                metrics
                    .connection_init_duplicate_joins_dropped
                    .observe(duplicate_joins_dropped as f64);
                metrics
                    .connection_init_joins_cancelled_by_part
                    .observe(joins_cancelled_by_part as f64);
            }
            send_to_client(
                &client_loop_tx,
                connection_id,
                ConnectionIncomingMessage::StateOpen,
            );
        }

        let mut outgoing = OutgoingSink::new(
            transport_outgoing,
            #[cfg(feature = "metrics-collection")]
            metrics.clone(),
        );

        outgoing.enqueue_message(
            irc!["CAP", "REQ", "twitch.tv/tags twitch.tv/commands"],
            None,
        );
        if let Some(token) = credentials.token {
            outgoing.enqueue_message(irc!["PASS", format!("oauth:{}", token)], None);
        }
        outgoing.enqueue_message(irc!["NICK", credentials.login], None);

        if duplicate_joins_dropped > 0 || joins_cancelled_by_part > 0 {
            tracing::debug!(
                queued_joins = queued_joins.len(),
                duplicate_joins_dropped,
                joins_cancelled_by_part,
                "Flushing coalesced JOINs on connection open"
            );
        }

        for channel in queued_joins {
            outgoing.enqueue_message(irc!["JOIN", channel], None);
        }

        for (message, return_sender) in commands_queue {
            outgoing.enqueue_message(message, return_sender);
        }

        // every 30 seconds we send out a PING
        // 5 seconds after sending it out, we check that we got a PONG message since sending that PING
        // if not, the connection is failed with an error (Error::PingTimeout)
        let ping_every = Duration::from_secs(30);
        let check_pong_after = Duration::from_secs(5);
        let mut send_ping_interval = interval_at(Instant::now() + ping_every, ping_every);
        let mut check_pong_interval =
            interval_at(Instant::now() + ping_every + check_pong_after, ping_every);
        let mut pong_received = false;

        let closure_cause = loop {
            let has_outgoing_work = outgoing.has_work();

            tokio::select! {
                biased;
                maybe_cmd = connection_loop_rx.recv() => {
                    match maybe_cmd {
                        Some(ConnectionLoopCommand::SendMessage(message, reply_sender)) => {
                            outgoing.enqueue_message(message, reply_sender);
                        }
                        None => break None,
                    }
                }
                maybe_incoming = transport_incoming.next() => {
                    match maybe_incoming {
                        None => {
                            tracing::trace!("EOF from transport");
                            tracing::info!("EOF received from transport incoming stream");
                            break Some(Error::RemoteUnexpectedlyClosedConnection);
                        }
                        Some(Err(err)) => {
                            let err = match err {
                                Either::Left(e) => Error::IncomingError(Arc::new(e)),
                                Either::Right(e) => Error::IRCParseError(e),
                            };
                            tracing::trace!("Error from transport: {}", err);
                            tracing::error!("Error received from transport incoming stream: {}", err);
                            break Some(err);
                        }
                        Some(Ok(irc_message)) => {
                            tracing::trace!("< {}", irc_message.as_raw_irc());
                            #[cfg(feature = "metrics-collection")]
                            if let Some(ref metrics) = metrics {
                                metrics
                                    .messages_received
                                    .with_label_values(&[&irc_message.command])
                                    .inc();
                            }

                            // Note! An error here (failing to parse to a ServerMessage) will not result
                            // in a connection abort. This is by design. See for example
                            // https://github.com/robotty/dank-twitch-irc/issues/22.
                            // The message will just be ignored instead
                            match ServerMessage::try_from(irc_message) {
                                Ok(server_message) => {
                                    send_to_client(
                                        &client_loop_tx,
                                        connection_id,
                                        ConnectionIncomingMessage::IncomingMessage(Box::new(
                                            server_message.clone(),
                                        )),
                                    );

                                    match &server_message {
                                        ServerMessage::Ping(_) => {
                                            outgoing.enqueue_message(
                                                irc!["PONG", "tmi.twitch.tv"],
                                                None,
                                            );
                                        }
                                        ServerMessage::Pong(_) => {
                                            tracing::trace!("Received pong");
                                            pong_received = true;
                                        }
                                        ServerMessage::Reconnect(_) => {
                                            break Some(Error::ReconnectCmd);
                                        }
                                        _ => {}
                                    }
                                }
                                Err(parse_error) => {
                                    tracing::error!(
                                        "Failed to parse incoming message as ServerMessage (emitting as generic instead): {}",
                                        parse_error
                                    );
                                    send_to_client(
                                        &client_loop_tx,
                                        connection_id,
                                        ConnectionIncomingMessage::IncomingMessage(Box::new(
                                            ServerMessage::new_generic(IRCMessage::from(
                                                parse_error,
                                            )),
                                        )),
                                    );
                                }
                            }
                        }
                    }
                }
                _ = send_ping_interval.tick() => {
                    tracing::trace!("sending ping");
                    pong_received = false;
                    outgoing.enqueue_message(irc!["PING", "tmi.twitch.tv"], None);
                }
                _ = check_pong_interval.tick() => {
                    tracing::trace!("checking for pong");
                    if !pong_received {
                        break Some(Error::PingTimeout);
                    }
                }
                res = outgoing.drive_once(&config.join_rate_limiter, &config.part_rate_limiter), if has_outgoing_work => {
                    if let Err(err) = res {
                        break Some(Error::OutgoingError(err));
                    }
                }
            }
        };

        // Phase 3: Closed drain
        if let Some(cause) = closure_cause {
            tracing::info!("Closing connection, cause: {}", cause);
            outgoing.fail_all_pending(&cause);
            drop(outgoing);
            drop(transport_incoming);

            send_to_client(
                &client_loop_tx,
                connection_id,
                ConnectionIncomingMessage::StateClosed {
                    cause: cause.clone(),
                },
            );

            Self::drain_closed(&mut connection_loop_rx, cause).await;
        }

        tracing::debug!("Connection event loop ended");
    }

    async fn drain_closed<T: Transport, L: LoginCredentials>(
        connection_loop_rx: &mut mpsc::UnboundedReceiver<ConnectionLoopCommand<T, L>>,
        cause: Error<T, L>,
    ) {
        while let Some(command) = connection_loop_rx.recv().await {
            match command {
                ConnectionLoopCommand::SendMessage(_message, Some(reply_sender)) => {
                    reply_sender.send(Err(cause.clone())).ok();
                }
                ConnectionLoopCommand::SendMessage(_message, None) => {}
            }
        }
    }

    async fn init_transport<T: Transport, L: LoginCredentials>(
        config: Arc<ClientConfig<L>>,
    ) -> Result<(T, CredentialsPair), Error<T, L>> {
        tracing::debug!("Starting connection init");
        let credentials = config
            .login_credentials
            .get_credentials()
            .await
            .map_err(Arc::new)
            .map_err(Error::LoginError)?;

        // rate limits the opening of new connections
        tracing::trace!("Trying to acquire permit for opening transport...");
        let rate_limit_permit = Arc::clone(&config.connection_rate_limiter)
            .acquire_owned()
            .await;
        tracing::trace!("Successfully got permit to open transport.");

        let connect_attempt = T::new();
        let timeout = tokio::time::sleep(config.connect_timeout);

        let transport_res = tokio::select! {
            t_result = connect_attempt => {
                t_result.map_err(Arc::new)
                    .map_err(Error::ConnectError)
            },
            () = timeout => {
                Err(Error::ConnectTimeout)
            }
        };

        // release the rate limit permit after the connect attempt and after
        // the specified time has elapsed.
        let new_connection_every = config.new_connection_every;
        tokio::spawn(
            async move {
                tokio::time::sleep(new_connection_every).await;
                drop(rate_limit_permit);
                tracing::trace!("Successfully released permit after waiting specified duration.");
            }
            .instrument(debug_span!("release_permit_task")),
        );

        // error return happens AFTER rate limit, otherwise we'd drop the rate_limit_permit
        // immediately instead of later after a delay.
        let transport = transport_res?;

        Ok((transport, credentials))
    }
}

#[cfg(test)]
mod tests {
    use super::ConnectionLoopWorker;
    use crate::client::event_loop::ClientLoopCommand;
    use crate::connection::{Connection, ConnectionIncomingMessage};
    use crate::{
        ClientConfig, RateLimiter, irc,
        login::StaticLoginCredentials,
        message::{IRCMessage, IRCParseError, ServerMessage},
        transport::Transport,
    };
    use async_trait::async_trait;
    use either::Either;
    use futures_util::{sink, stream};
    use std::{
        convert::Infallible,
        future,
        pin::Pin,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        task::{Context, Poll, Waker},
        time::Duration,
    };
    use tokio::sync::{Semaphore, mpsc};

    #[derive(Debug)]
    struct FailingTransport;

    #[async_trait]
    impl Transport for FailingTransport {
        type ConnectError = &'static str;
        type IncomingError = Infallible;
        type OutgoingError = Infallible;
        type Incoming =
            stream::Empty<Result<IRCMessage, Either<Self::IncomingError, IRCParseError>>>;
        type Outgoing = sink::Drain<IRCMessage>;

        async fn new() -> Result<Self, Self::ConnectError> {
            Err("deliberate connection failure")
        }

        fn split(self) -> (Self::Incoming, Self::Outgoing) {
            (stream::empty(), sink::drain())
        }
    }

    #[derive(Debug)]
    struct PendingTransport;

    #[async_trait]
    impl Transport for PendingTransport {
        type ConnectError = Infallible;
        type IncomingError = Infallible;
        type OutgoingError = Infallible;
        type Incoming =
            stream::Empty<Result<IRCMessage, Either<Self::IncomingError, IRCParseError>>>;
        type Outgoing = sink::Drain<IRCMessage>;

        async fn new() -> Result<Self, Self::ConnectError> {
            future::pending().await
        }

        fn split(self) -> (Self::Incoming, Self::Outgoing) {
            (stream::empty(), sink::drain())
        }
    }

    type IncomingItem = Result<IRCMessage, Either<Infallible, IRCParseError>>;

    #[derive(Debug)]
    struct StalledFlushIncoming {
        rx: mpsc::UnboundedReceiver<IncomingItem>,
        terminated: bool,
    }

    impl futures_util::Stream for StalledFlushIncoming {
        type Item = IncomingItem;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            if self.terminated {
                return Poll::Ready(None);
            }
            match self.rx.poll_recv(cx) {
                Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
                Poll::Ready(None) => {
                    self.terminated = true;
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }

    impl futures_util::stream::FusedStream for StalledFlushIncoming {
        fn is_terminated(&self) -> bool {
            self.terminated
        }
    }

    #[derive(Debug)]
    struct FlushGate {
        allowed: AtomicBool,
        waker: Mutex<Option<Waker>>,
    }

    impl FlushGate {
        fn new() -> Self {
            FlushGate {
                allowed: AtomicBool::new(false),
                waker: Mutex::new(None),
            }
        }

        fn unblock(&self) {
            self.allowed.store(true, Ordering::Release);
            if let Some(waker) = self.waker.lock().unwrap().take() {
                waker.wake();
            }
        }
    }

    #[derive(Debug)]
    struct StalledFlushOutgoing {
        tx: mpsc::UnboundedSender<IRCMessage>,
        flush_after_handshake: bool,
        saw_nick: bool,
        gate: Arc<FlushGate>,
    }

    impl sink::Sink<IRCMessage> for StalledFlushOutgoing {
        type Error = Infallible;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(mut self: Pin<&mut Self>, item: IRCMessage) -> Result<(), Self::Error> {
            if item.command == "NICK" {
                self.saw_nick = true;
            } else if self.saw_nick {
                self.flush_after_handshake = true;
            }
            let _ = self.tx.send(item);
            Ok(())
        }

        fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            if !self.flush_after_handshake || self.gate.allowed.load(Ordering::Acquire) {
                Poll::Ready(Ok(()))
            } else {
                *self.gate.waker.lock().unwrap() = Some(cx.waker().clone());
                if self.gate.allowed.load(Ordering::Acquire) {
                    Poll::Ready(Ok(()))
                } else {
                    Poll::Pending
                }
            }
        }

        fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.poll_flush(cx)
        }
    }

    struct StalledFlushTestHandles {
        incoming_rx: mpsc::UnboundedReceiver<IncomingItem>,
        outgoing_tx: mpsc::UnboundedSender<IRCMessage>,
        gate: Arc<FlushGate>,
    }

    static STALLED_FLUSH_HANDLES: Mutex<Option<StalledFlushTestHandles>> = Mutex::new(None);

    #[derive(Debug)]
    struct StalledFlushTransport {
        incoming: StalledFlushIncoming,
        outgoing: StalledFlushOutgoing,
    }

    #[async_trait]
    impl Transport for StalledFlushTransport {
        type ConnectError = Infallible;
        type IncomingError = Infallible;
        type OutgoingError = Infallible;
        type Incoming = StalledFlushIncoming;
        type Outgoing = StalledFlushOutgoing;

        async fn new() -> Result<Self, Self::ConnectError> {
            let handles = STALLED_FLUSH_HANDLES.lock().unwrap().take().unwrap();
            Ok(StalledFlushTransport {
                incoming: StalledFlushIncoming {
                    rx: handles.incoming_rx,
                    terminated: false,
                },
                outgoing: StalledFlushOutgoing {
                    tx: handles.outgoing_tx,
                    flush_after_handshake: false,
                    saw_nick: false,
                    gate: handles.gate,
                },
            })
        }

        fn split(self) -> (Self::Incoming, Self::Outgoing) {
            (self.incoming, self.outgoing)
        }
    }

    #[tokio::test]
    async fn failed_connection_attempts_are_rate_limited() {
        let config = Arc::new(ClientConfig {
            new_connection_every: Duration::from_secs(60),
            ..ClientConfig::default()
        });
        let connection_rate_limiter = Arc::clone(&config.connection_rate_limiter);

        let res = ConnectionLoopWorker::init_transport::<FailingTransport, StaticLoginCredentials>(
            Arc::clone(&config),
        )
        .await;

        assert!(res.is_err());
        assert_eq!(connection_rate_limiter.available_permits(), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn connection_cooldown_starts_after_transport_timeout() {
        let connect_timeout = Duration::from_secs(10);
        let new_connection_every = Duration::from_secs(20);
        let config = Arc::new(ClientConfig {
            connect_timeout,
            new_connection_every,
            ..ClientConfig::default()
        });
        let connection_rate_limiter = Arc::clone(&config.connection_rate_limiter);
        let started_at = tokio::time::Instant::now();

        let res = ConnectionLoopWorker::init_transport::<PendingTransport, StaticLoginCredentials>(
            config,
        )
        .await;

        assert!(matches!(res, Err(crate::Error::ConnectTimeout)));
        assert_eq!(started_at.elapsed(), connect_timeout);
        assert_eq!(connection_rate_limiter.available_permits(), 0);

        tokio::task::yield_now().await;
        tokio::time::advance(new_connection_every - Duration::from_secs(1)).await;
        assert_eq!(connection_rate_limiter.available_permits(), 0);

        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(connection_rate_limiter.available_permits(), 1);
    }

    #[tokio::test]
    async fn incoming_messages_and_ping_proceed_while_outgoing_flush_is_stalled() {
        let (in_tx, in_rx) = mpsc::unbounded_channel();
        let (out_tx, mut out_rx) = mpsc::unbounded_channel();
        let gate = Arc::new(FlushGate::new());
        *STALLED_FLUSH_HANDLES.lock().unwrap() = Some(StalledFlushTestHandles {
            incoming_rx: in_rx,
            outgoing_tx: out_tx,
            gate: Arc::clone(&gate),
        });

        let config = Arc::new(ClientConfig {
            connection_rate_limiter: Arc::new(Semaphore::new(10)),
            new_connection_every: Duration::ZERO,
            join_rate_limiter: Arc::new(RateLimiter::unlimited()),
            part_rate_limiter: Arc::new(RateLimiter::unlimited()),
            #[cfg(feature = "metrics-collection")]
            metrics_config: crate::MetricsConfig::Disabled,
            ..ClientConfig::default()
        });
        let (client_loop_tx, mut client_loop_rx) = mpsc::unbounded_channel();
        let client_loop_tx = Arc::new(client_loop_tx);

        let conn = Connection::<StalledFlushTransport, StaticLoginCredentials>::new(
            config,
            0,
            Arc::downgrade(&client_loop_tx),
            #[cfg(feature = "metrics-collection")]
            None,
        );

        // Send a reply-tracked message whose poll_flush will block on `gate`.
        let (reply_tx, mut reply_rx) = tokio::sync::oneshot::channel();
        conn.connection_loop_tx
            .send(super::ConnectionLoopCommand::SendMessage(
                irc!["PRIVMSG", "#chan", "blocked flush"],
                Some(reply_tx),
            ))
            .unwrap();

        // Wait until CAP, NICK, and the PRIVMSG have all been submitted via start_send.
        for expected in ["CAP", "NICK", "PRIVMSG"] {
            let sent = out_rx.recv().await.unwrap();
            assert_eq!(sent.command, expected);
        }
        tokio::task::yield_now().await;
        assert!(reply_rx.try_recv().is_err());

        // While poll_flush is stalled, incoming PING + JOIN must still be processed immediately.
        in_tx
            .send(Ok(IRCMessage::parse("PING :tmi.twitch.tv").unwrap()))
            .unwrap();
        in_tx
            .send(Ok(IRCMessage::parse(
                ":alice!alice@alice.tmi.twitch.tv JOIN #chan",
            )
            .unwrap()))
            .unwrap();

        for expected in ["Ping", "Join"] {
            loop {
                match client_loop_rx.recv().await.unwrap() {
                    ClientLoopCommand::IncomingMessage { message, .. } => {
                        if let ConnectionIncomingMessage::IncomingMessage(server_msg) = *message {
                            match (&*server_msg, expected) {
                                (ServerMessage::Ping(_), "Ping")
                                | (ServerMessage::Join(_), "Join") => break,
                                other => panic!("unexpected server message {other:?}"),
                            }
                        }
                    }
                    other => panic!("unexpected client command {other:?}"),
                }
            }
        }

        // The Ping handler also submitted PONG via start_send without waiting on the stalled flush.
        let pong = out_rx.recv().await.unwrap();
        assert_eq!(pong.command, "PONG");
        assert!(reply_rx.try_recv().is_err());

        // Unblock poll_flush and verify the pending PRIVMSG reply completes cleanly.
        gate.unblock();
        reply_rx.await.unwrap().unwrap();
    }
}
