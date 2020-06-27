use crate::config::{ClientConfig, CredentialsPair, LoginCredentials};
use crate::connection::error::ConnectionError;
use crate::irc;
use crate::message::commands::ServerMessage;
use crate::message::AsRawIRC;
use crate::message::IRCMessage;
use crate::transport::Transport;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures::stream;
use futures::stream::FusedStream;
use itertools::Itertools;
use smallvec::SmallVec;
use std::collections::{HashSet, VecDeque};
use std::convert::TryFrom;
use std::mem;
use std::ops::RangeFull;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::{interval_at, Duration, Instant};

pub(crate) enum ConnectionLoopCommand<T: Transport<L>, L: LoginCredentials> {
    // commands that come from Connection methods
    SendMessage(
        IRCMessage,
        Option<oneshot::Sender<Result<(), ConnectionError<T, L>>>>,
    ),
    Join(String, oneshot::Sender<Result<(), ConnectionError<T, L>>>),
    Part(String, oneshot::Sender<Result<(), ConnectionError<T, L>>>),
    Close(Option<ConnectionError<T, L>>, Option<oneshot::Sender<()>>),

    // commands that come from the outgoing loop
    TransportInitFinished(Result<T, ConnectionError<T, L>>),
    SendError(T::OutgoingError),

    // commands that come from the incoming loop
    IncomingMessage(Option<Result<IRCMessage, ConnectionError<T, L>>>),

    // commands that come from the ping loop
    SendPing(),
    CheckPong(),
}

enum ConnectionLoopState<T: Transport<L>, L: LoginCredentials> {
    Initializing {
        channels: HashSet<String>,
        commands_queue: VecDeque<ConnectionLoopCommand<T, L>>,
        connection_loop_tx: mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>,
        connection_incoming_tx: mpsc::UnboundedSender<Result<ServerMessage, ConnectionError<T, L>>>,
    },
    Open {
        transport_outgoing: Arc<Mutex<T::Outgoing>>,
        channels: HashSet<String>,
        connection_loop_tx: mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>,
        connection_incoming_tx: mpsc::UnboundedSender<Result<ServerMessage, ConnectionError<T, L>>>,
        tx_kill_incoming: oneshot::Sender<()>,
        tx_kill_pinger: oneshot::Sender<()>,
        pong_received: bool,
    },
    Closed,
}

pub(crate) struct ConnectionLoopWorker<T: Transport<L>, L: LoginCredentials> {
    config: Arc<ClientConfig<L>>,
    connection_loop_rx: mpsc::UnboundedReceiver<ConnectionLoopCommand<T, L>>,
    state: ConnectionLoopState<T, L>,
}

impl<T: Transport<L>, L: LoginCredentials> ConnectionLoopWorker<T, L> {
    pub fn new(
        config: Arc<ClientConfig<L>>,
        connection_incoming_tx: mpsc::UnboundedSender<Result<ServerMessage, ConnectionError<T, L>>>,
        connection_loop_tx: mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>,
        connection_loop_rx: mpsc::UnboundedReceiver<ConnectionLoopCommand<T, L>>,
    ) -> ConnectionLoopWorker<T, L> {
        ConnectionLoopWorker {
            config,
            connection_loop_rx,
            state: ConnectionLoopState::Initializing {
                channels: HashSet::new(),
                commands_queue: VecDeque::new(),
                connection_loop_tx,
                connection_incoming_tx,
            },
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    fn spawn_init_task(&self) -> JoinHandle<()> {
        let config = Arc::clone(&self.config);

        // extract a clone of connection_loop_tx from self.state
        let connection_loop_tx = if let ConnectionLoopState::Initializing {
            ref connection_loop_tx,
            ..
        } = &self.state
        {
            connection_loop_tx.clone()
        } else {
            panic!("spawn_init_task expects a state of Initializing")
        };

        tokio::spawn(async move {
            log::debug!("Spawned connection init task");
            let res = async {
                let credentials = config
                    .login_credentials
                    .get_credentials()
                    .await
                    .map_err(ConnectionError::<T, L>::LoginError)?;

                let mut transport = T::new()
                    .await
                    .map_err(ConnectionError::<T, L>::ConnectError)?;

                let mut commands = SmallVec::<[IRCMessage; 3]>::new();
                commands.push(irc!["CAP", "REQ", "twitch.tv/tags twitch.tv/commands"]);

                let CredentialsPair { login, token } = credentials;
                if let Some(token) = token {
                    commands.push(irc!["PASS", format!("oauth:{}", token)]);
                }
                commands.push(irc!["NICK", login]);

                // note on the goofy `map()` call here: send_all expects to be fed with a
                // TryStream, so we wrap all elements in Ok().
                // Additionally, the fed-in Result objects have to have the same error type
                // as the error of the stream, which is why we cannot use `Infallible` or similar
                // more descript type here.
                transport
                    .outgoing()
                    .send_all(&mut stream::iter(
                        commands.into_iter().map(Ok::<IRCMessage, T::OutgoingError>),
                    ))
                    .await
                    .map_err(ConnectionError::<T, L>::OutgoingError)?;

                Ok::<T, ConnectionError<T, L>>(transport)
            }
            .await;

            // res is now the result of the init work
            connection_loop_tx
                .unbounded_send(ConnectionLoopCommand::TransportInitFinished(res))
                .unwrap();
        })
    }

    async fn run(mut self) {
        log::debug!("Spawned connection event loop");
        self.spawn_init_task();

        while let Some(command) = self.connection_loop_rx.next().await {
            self.process_command(command);
        }
        log::debug!("Connection event loop ended")
    }

    fn process_command(&mut self, command: ConnectionLoopCommand<T, L>) {
        match command {
            ConnectionLoopCommand::SendMessage(message, reply_sender) => {
                self.send_message(message, reply_sender);
            }
            ConnectionLoopCommand::Join(channel, reply_sender) => {
                self.join(channel, reply_sender);
            }
            ConnectionLoopCommand::Part(channel, reply_sender) => {
                self.part(channel, reply_sender);
            }
            ConnectionLoopCommand::Close(maybe_err, reply_sender) => {
                self.transition_to_closed(maybe_err);
                if let Some(reply_sender) = reply_sender {
                    reply_sender.send(()).ok();
                }
            }
            ConnectionLoopCommand::TransportInitFinished(init_result) => {
                self.on_transport_init_finished(init_result);
            }
            ConnectionLoopCommand::SendError(error) => {
                self.on_send_error(error);
            }
            ConnectionLoopCommand::IncomingMessage(maybe_msg) => {
                self.on_incoming_message(maybe_msg);
            }
            ConnectionLoopCommand::SendPing() => self.send_ping(),
            ConnectionLoopCommand::CheckPong() => self.check_pong(),
        };
    }

    fn send_message(
        &mut self,
        message: IRCMessage,
        reply_sender: Option<oneshot::Sender<Result<(), ConnectionError<T, L>>>>,
    ) {
        match &mut self.state {
            ConnectionLoopState::Initializing {
                ref mut commands_queue,
                ..
            } => {
                commands_queue.push_back(ConnectionLoopCommand::SendMessage(message, reply_sender));
            }
            ConnectionLoopState::Open {
                ref transport_outgoing,
                ref connection_loop_tx,
                ..
            } => {
                let transport_outgoing = Arc::clone(&transport_outgoing);
                let connection_loop_tx = connection_loop_tx.clone();
                tokio::spawn(async move {
                    let mut transport_outgoing = transport_outgoing.lock().await;
                    log::trace!("> {}", message.as_raw_irc());
                    let res = transport_outgoing.send(message).await;

                    // The error is cloned and sent both to the calling method as well as
                    // the connection event loop so it can end with that error.
                    if let Some(reply_sender) = reply_sender {
                        reply_sender
                            .send(res.clone().map_err(ConnectionError::<T, L>::OutgoingError))
                            .ok();
                    }
                    if let Err(err) = res {
                        connection_loop_tx
                            .unbounded_send(ConnectionLoopCommand::SendError(err))
                            .unwrap();
                        // unwrap: connection loop should not die before all of its senders
                        // are dropped.
                    }
                });
            }
            ConnectionLoopState::Closed => {
                if let Some(reply_sender) = reply_sender {
                    reply_sender
                        .send(Err(ConnectionError::<T, L>::ConnectionClosed()))
                        .ok();
                }
            }
        }
    }

    fn join(
        &mut self,
        channel: String,
        reply_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    ) {
        match &mut self.state {
            ConnectionLoopState::Initializing {
                ref mut channels, ..
            }
            | ConnectionLoopState::Open {
                ref mut channels, ..
            } => {
                channels.insert(channel.clone());
                self.send_message(irc!["JOIN", format!("#{}", channel)], Some(reply_sender));
            }
            ConnectionLoopState::Closed => {
                reply_sender
                    .send(Err(ConnectionError::<T, L>::ConnectionClosed()))
                    .ok();
            }
        }
    }

    fn part(
        &mut self,
        channel: String,
        reply_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    ) {
        match &mut self.state {
            ConnectionLoopState::Initializing {
                ref mut channels, ..
            }
            | ConnectionLoopState::Open {
                ref mut channels, ..
            } => {
                channels.remove(&channel);
                self.send_message(irc!["PART", format!("#{}", channel)], Some(reply_sender));
            }
            ConnectionLoopState::Closed => {
                reply_sender
                    .send(Err(ConnectionError::<T, L>::ConnectionClosed()))
                    .ok();
            }
        }
    }

    /// Transitions this worker into the `Closed` state.
    ///
    /// Overview of states for this worker:
    ///
    /// We start out in `Initializing`.
    /// Possible paths are only:
    /// 1) Initializing -> Open -> Closed
    /// 2) Initializing -> Closed
    /// This method covers the `-> Closed` part.
    /// `-> Open` is covered by `on_transport_init_finished`.
    ///
    /// `err` is an optional error to emit before closing the stream. If `err` is `None`,
    /// the stream will be closed without emitting an `Err` message before-hand.
    fn transition_to_closed(&mut self, err: Option<ConnectionError<T, L>>) {
        log::info!("Closing connection, reason: {:?}", err);
        let old_state = mem::replace(&mut self.state, ConnectionLoopState::Closed);

        match old_state {
            ConnectionLoopState::Initializing {
                mut commands_queue,
                connection_incoming_tx,
                ..
            } => {
                for command in commands_queue.drain(RangeFull) {
                    self.process_command(command);
                }

                if let Some(err) = err {
                    // .ok(): ignore error if receiver is disconnected
                    connection_incoming_tx.unbounded_send(Err(err)).ok();
                }
            }
            ConnectionLoopState::Open {
                connection_incoming_tx,
                tx_kill_incoming,
                tx_kill_pinger,
                ..
            } => {
                if let Some(err) = err {
                    // .ok(): ignore error if receiver is disconnected
                    connection_incoming_tx.unbounded_send(Err(err)).ok();
                }

                tx_kill_incoming.send(()).ok();
                tx_kill_pinger.send(()).ok();
            }
            ConnectionLoopState::Closed => {}
        }
    }

    fn on_transport_init_finished(&mut self, init_result: Result<T, ConnectionError<T, L>>) {
        match &mut self.state {
            ConnectionLoopState::Initializing {
                ref mut channels,
                ref mut commands_queue,
                ref connection_loop_tx,
                ref connection_incoming_tx,
            } => {
                // .drain().collect() makes a new collection without cloning
                // the elements. We can't directly move these collections out
                // of the ConnectionLoopState::Initializing variant, so we have to make a new
                // collection while leaving an empty collection behind (draining the elements
                // into a new collection)
                let mut commands_queue = commands_queue.drain(RangeFull).collect_vec();

                match init_result {
                    Ok(transport) => {
                        // transport was opened successfully
                        log::info!(
                            "Transport init task has finished, transitioning to Initializing"
                        );
                        let (transport_incoming, transport_outgoing) = transport.split();

                        let (tx_kill_incoming, rx_kill_incoming) = oneshot::channel();
                        ConnectionLoopWorker::spawn_incoming_forward_task(
                            transport_incoming,
                            connection_loop_tx.clone(),
                            rx_kill_incoming,
                        );

                        let (tx_kill_pinger, rx_kill_pinger) = oneshot::channel();
                        ConnectionLoopWorker::spawn_ping_task(
                            connection_loop_tx.clone(),
                            rx_kill_pinger,
                        );

                        // transition our own state from Initializing to Open
                        self.state = ConnectionLoopState::Open {
                            transport_outgoing: Arc::new(Mutex::new(transport_outgoing)),
                            channels: channels.drain().collect(),
                            connection_loop_tx: connection_loop_tx.clone(),
                            connection_incoming_tx: connection_incoming_tx.clone(),
                            tx_kill_incoming,
                            tx_kill_pinger,
                            pong_received: false,
                        };
                    }
                    Err(init_error) => {
                        // emit error to downstream + transition to closed
                        log::info!(
                            "Transport init task has finished with error, closing connection"
                        );
                        self.transition_to_closed(Some(init_error));
                    }
                };

                // then process the commands_queue backlog before returning,
                // the relevant event handlers will deal with what happens depending on
                // whether the state is now Open or Closed
                for command in commands_queue.drain(RangeFull) {
                    self.process_command(command);
                }
            }
            ConnectionLoopState::Open { .. } => {
                unreachable!("on_transport_init_finished must never be called more than once");
            }
            ConnectionLoopState::Closed { .. } => {}
        }
    }

    fn spawn_incoming_forward_task(
        mut transport_incoming: T::Incoming,
        connection_loop_tx: mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>,
        mut rx_kill_incoming: oneshot::Receiver<()>,
    ) -> JoinHandle<()>
    where
        T::Incoming: FusedStream,
    {
        tokio::spawn(async move {
            log::debug!("Spawned incoming messages forwarder");
            loop {
                tokio::select! {
                    _ = &mut rx_kill_incoming => {
                        // got kill signal
                        break;
                    }
                    incoming_message = transport_incoming.next() => {
                        let do_exit = matches!(incoming_message, None | Some(Err(_)));
                        // unwrap(): We don't expect the connection loop to die before all tx clones
                        // are dropped (and we are holding one right now)
                        connection_loop_tx.unbounded_send(ConnectionLoopCommand::IncomingMessage(incoming_message)).unwrap();
                        if do_exit {
                            break;
                        }
                    }
                }
            }
            log::debug!("Incoming messages forwarder ended");
        })
    }

    fn spawn_ping_task(
        connection_loop_tx: mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>,
        mut rx_kill_pinger: oneshot::Receiver<()>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            log::debug!("Spawned pinger task");
            // every 30 seconds we send out a PING
            // 5 seconds after sending it out, we check that we got a PONG message since sending that PING
            // if not, the connection is failed with an error (ConnectionError::PingTimeout)
            let ping_every = Duration::from_secs(30);
            let check_pong_after = Duration::from_secs(5);

            let mut send_ping_interval = interval_at(Instant::now() + ping_every, ping_every);
            let mut check_pong_interval =
                interval_at(Instant::now() + ping_every + check_pong_after, ping_every);

            loop {
                tokio::select! {
                    _ = &mut rx_kill_pinger => {
                        break;
                    },
                    _ = send_ping_interval.tick() => {
                        log::debug!("sending ping");
                        connection_loop_tx.unbounded_send(ConnectionLoopCommand::SendPing()).unwrap();
                    }
                    _ = check_pong_interval.tick() => {
                        log::debug!("checking for pong");
                        connection_loop_tx.unbounded_send(ConnectionLoopCommand::CheckPong()).unwrap();
                    }
                }
            }
        })
    }

    fn send_ping(&mut self) {
        // invoked by the pinger task. Send out a `PING` message, and reset the `pong_received` variable.
        match &mut self.state {
            ConnectionLoopState::Initializing { .. } => {
                panic!("unexpected SendPing message while in state `Initializing`")
            }
            ConnectionLoopState::Open {
                ref mut pong_received,
                ..
            } => {
                *pong_received = false;
                self.send_message(irc!["PING", "tmi.twitch.tv"], None);
            }
            ConnectionLoopState::Closed => {} // do nothing
        }
    }

    fn check_pong(&mut self) {
        // invoked by the pinger task. Check if `pong_received` is true, otherwise fail the connection.
        match &mut self.state {
            ConnectionLoopState::Initializing { .. } => {
                panic!("unexpected CheckPong message while in state `Initializing`")
            }
            ConnectionLoopState::Open {
                ref mut pong_received,
                ..
            } => {
                if !*pong_received {
                    self.transition_to_closed(Some(ConnectionError::<T, L>::PingTimeout()))
                }
            }
            ConnectionLoopState::Closed => {} // do nothing
        }
    }

    fn on_send_error(&mut self, error: T::OutgoingError) {
        self.transition_to_closed(Some(ConnectionError::<T, L>::OutgoingError(error)))
    }

    fn on_incoming_message(
        &mut self,
        maybe_message: Option<Result<IRCMessage, ConnectionError<T, L>>>,
    ) {
        if matches!(self.state, ConnectionLoopState::Initializing { .. }) {
            panic!("unexpected incoming message while still initializing")
        }

        match maybe_message {
            None => {
                log::info!("EOF received from transport incoming stream");
                self.transition_to_closed(Some(ConnectionError::<T, L>::ConnectionClosed()));
            }
            Some(Err(error)) => {
                log::error!("Error received from transport incoming stream: {:?}", error);
                self.transition_to_closed(Some(error));
            }
            Some(Ok(irc_message)) => {
                log::trace!("< {}", irc_message.as_raw_irc());

                // FORWARD MESSAGE.
                // we forward the message, but before that, make a copy of it if the parsing was
                // successful (.as_ref().ok().cloned()) and then return that for further processing.
                // the message handling happens after the forward because we might want to alter
                // the state or send messages as a result of the handling, which should come after
                // the message is forwarded (e.g. RECONNECT message - first the RECONNECT
                // should be received by the downstream, then the error event, then the EOF).
                // If we first did all of that, we would end up not forwarding the RECONNECT at all
                // because at that point the client would already be in state `Closed` and not able
                // to forward anything anymore.
                let msg_if_ok = if let ConnectionLoopState::Open {
                    ref connection_incoming_tx,
                    ..
                } = &self.state
                {
                    // Note! An error here (failing to parse to a ServerMessage) will not result
                    // in a connection abort. This is by design. See for example
                    // https://github.com/robotty/dank-twitch-irc/issues/22.
                    let server_message = ServerMessage::try_from(irc_message.clone())
                        .map_err(ConnectionError::<T, L>::ServerMessageParseError);

                    // forward the message
                    // if the message either (a) did not parse as Generic or (b) failed to parse
                    // we emit it as a Generic additionally so you can use the ServerMessage::Generic
                    // type as a catch-all (for case a), and so the downstream can still receive
                    // messages that failed to parse as ServerMessage (for case b)
                    if !matches!(server_message, Ok(ServerMessage::Generic(_))) {
                        connection_incoming_tx
                            .unbounded_send(Ok(ServerMessage::Generic(irc_message)))
                            .ok();
                    }

                    let msg_if_ok = server_message.as_ref().ok().cloned();

                    connection_incoming_tx.unbounded_send(server_message).ok();

                    msg_if_ok
                } else {
                    // state `Closed`, no need to further process any messages
                    None
                };

                // HANDLE MESSAGE.
                // return if there is nothing to handle
                let server_message = match msg_if_ok {
                    Some(server_message) => server_message,
                    None => return,
                };

                if let ConnectionLoopState::Open {
                    ref mut pong_received,
                    ..
                } = &mut self.state
                {
                    // react to PING and RECONNECT
                    match &server_message {
                        ServerMessage::Ping(_) => {
                            self.send_message(irc!["PONG", "tmi.twitch.tv"], None);
                        }
                        ServerMessage::Pong(_) => {
                            log::trace!("Received pong");
                            *pong_received = true;
                        }
                        ServerMessage::Reconnect(_) => {
                            // disconnect
                            self.transition_to_closed(
                                Some(ConnectionError::<T, L>::ReconnectCmd()),
                            );
                        }
                        _ => {}
                    }
                }
                // else: state is Closed, messages will not be forwarded nor replied to
            }
        }
    }
}
