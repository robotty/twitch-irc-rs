use crate::config::ClientConfig;
use crate::connection::ConnectionIncomingMessage;
use crate::error::Error;
use crate::irc;
use crate::login::{CredentialsPair, LoginCredentials};
use crate::message::commands::ServerMessage;
use crate::message::IRCMessage;
use crate::transport::{Transport, TransportStream};
use enum_dispatch::enum_dispatch;
use futures::prelude::*;
use itertools::Either;
use std::collections::VecDeque;
use std::convert::TryFrom;
use std::sync::{Arc, Weak};
use tokio::sync::oneshot::Sender;
use tokio::sync::Mutex;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{interval_at, Duration, Instant};

#[derive(Debug)]
pub(crate) enum ConnectionLoopCommand<T: Transport, L: LoginCredentials> {
    // commands that come from Connection methods
    SendMessage(IRCMessage, Option<oneshot::Sender<Result<(), Error<T, L>>>>),

    // comes from the init task
    TransportInitFinished(Result<(TransportStream<T>, CredentialsPair), Error<T, L>>),

    // comes from the task(s) spawned when a message is sent
    SendError(T::OutgoingError),

    // commands that come from the incoming loop
    // Some(Ok(_)) is an ordinary message, Some(Err(_)) an error, and None an EOF (end of stream)
    IncomingMessage(Option<Result<IRCMessage, Error<T, L>>>),

    // commands that come from the ping loop
    SendPing(),
    CheckPong(),
}

#[enum_dispatch]
trait ConnectionLoopStateMethods<T: Transport, L: LoginCredentials> {
    fn send_message(
        &mut self,
        message: IRCMessage,
        reply_sender: Option<oneshot::Sender<Result<(), Error<T, L>>>>,
    );
    fn on_transport_init_finished(
        self,
        init_result: Result<(TransportStream<T>, CredentialsPair), Error<T, L>>,
    ) -> ConnectionLoopState<T, L>;
    fn on_send_error(self, error: T::OutgoingError) -> ConnectionLoopState<T, L>;
    fn on_incoming_message(
        self,
        maybe_message: Option<Result<IRCMessage, Error<T, L>>>,
    ) -> ConnectionLoopState<T, L>;
    fn send_ping(&mut self);
    fn check_pong(self) -> ConnectionLoopState<T, L>;
}

#[enum_dispatch(ConnectionLoopStateMethods)]
enum ConnectionLoopState<T: Transport, L: LoginCredentials> {
    Initializing(ConnectionLoopInitializingState<T, L>),
    Open(ConnectionLoopOpenState<T, L>),
    Closed(ConnectionLoopClosedState),
}

pub(crate) struct ConnectionLoopWorker<T: Transport, L: LoginCredentials> {
    connection_loop_rx: mpsc::UnboundedReceiver<ConnectionLoopCommand<T, L>>,
    state: ConnectionLoopState<T, L>,
}

impl<T: Transport, L: LoginCredentials> ConnectionLoopWorker<T, L> {
    pub fn spawn(
        config: Arc<ClientConfig<L>>,
        connection_incoming_tx: mpsc::UnboundedSender<ConnectionIncomingMessage<T, L>>,
        connection_loop_tx: Weak<mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>>,
        connection_loop_rx: mpsc::UnboundedReceiver<ConnectionLoopCommand<T, L>>,
    ) {
        let worker = ConnectionLoopWorker {
            connection_loop_rx,
            state: ConnectionLoopState::Initializing(ConnectionLoopInitializingState {
                commands_queue: VecDeque::new(),
                connection_loop_tx: Weak::clone(&connection_loop_tx),
                connection_incoming_tx,
            }),
        };

        tokio::spawn(ConnectionLoopWorker::run_init_task(
            config,
            connection_loop_tx,
        ));
        tokio::spawn(worker.run());
    }

    async fn run_init_task(
        config: Arc<ClientConfig<L>>,
        connection_loop_tx: Weak<mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>>,
    ) {
        log::debug!("Spawned connection init task");
        // async{}.await is used in place of a try block since they are not stabilized yet
        // TODO revise this once try blocks are stabilized
        let res = async {
            let credentials = config
                .login_credentials
                .get_credentials()
                .await
                .map_err(Error::LoginError)?;

            // rate limits the opening of new connections
            log::trace!("Trying to acquire permit for opening transport...");
            let rate_limit_permit = Arc::clone(&config.connection_rate_limiter)
                .acquire_owned()
                .await;
            log::trace!("Successfully got permit to open transport.");

            let transport = T::new(config.metrics_identifier.clone())
                .await
                .map_err(Error::ConnectError)?;

            // release the rate limit permit after the transport is connected and after
            // the specified time has elapsed.
            tokio::spawn(async move {
                tokio::time::delay_for(config.new_connection_every).await;
                drop(rate_limit_permit);
                log::trace!("Successfully released permit after waiting specified duration.");
            });

            Ok::<(TransportStream<T>, CredentialsPair), Error<T, L>>((transport, credentials))
        }
        .await;

        // res is now the result of the init work
        if let Some(connection_loop_tx) = connection_loop_tx.upgrade() {
            connection_loop_tx
                .send(ConnectionLoopCommand::TransportInitFinished(res))
                .unwrap();
        }
    }

    async fn run(mut self) {
        log::debug!("Spawned connection event loop");
        while let Some(command) = self.connection_loop_rx.next().await {
            self = self.process_command(command);
        }
        log::debug!("Connection event loop ended")
    }

    /// Process a command, consuming the current state and returning a new state
    fn process_command(mut self, command: ConnectionLoopCommand<T, L>) -> Self {
        match command {
            ConnectionLoopCommand::SendMessage(message, reply_sender) => {
                self.state.send_message(message, reply_sender);
            }
            ConnectionLoopCommand::TransportInitFinished(init_result) => {
                self.state = self.state.on_transport_init_finished(init_result);
            }
            ConnectionLoopCommand::SendError(error) => {
                self.state = self.state.on_send_error(error);
            }
            ConnectionLoopCommand::IncomingMessage(maybe_msg) => {
                self.state = self.state.on_incoming_message(maybe_msg);
            }
            ConnectionLoopCommand::SendPing() => self.state.send_ping(),
            ConnectionLoopCommand::CheckPong() => {
                self.state = self.state.check_pong();
            }
        };
        self
    }
}

//
// INITIALIZING STATE
//
struct ConnectionLoopInitializingState<T: Transport, L: LoginCredentials> {
    // a list of queued up ConnectionLoopCommand::SendMessage messages
    commands_queue: VecDeque<(IRCMessage, Option<oneshot::Sender<Result<(), Error<T, L>>>>)>,
    connection_loop_tx: Weak<mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>>,
    connection_incoming_tx: mpsc::UnboundedSender<ConnectionIncomingMessage<T, L>>,
}

impl<T: Transport, L: LoginCredentials> ConnectionLoopInitializingState<T, L> {
    fn transition_to_closed(self, err: Option<Error<T, L>>) -> ConnectionLoopState<T, L> {
        log::info!("Closing connection, reason: {:?}", err);

        for (_message, return_sender) in self.commands_queue.into_iter() {
            if let Some(return_sender) = return_sender {
                return_sender.send(Err(Error::ConnectionClosed)).ok();
            }
        }

        let err_to_send = err.unwrap_or(Error::ConnectionClosed);

        self.connection_incoming_tx
            .send(ConnectionIncomingMessage::StateClosed { cause: err_to_send })
            .ok();

        // return the new state the connection should take on
        ConnectionLoopState::Closed(ConnectionLoopClosedState)
    }

    async fn run_incoming_forward_task(
        mut transport_incoming: T::Incoming,
        connection_loop_tx: Weak<mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>>,
        mut shutdown_notify: oneshot::Receiver<()>,
    ) {
        log::debug!("Spawned incoming messages forwarder");
        loop {
            tokio::select! {
                _ = &mut shutdown_notify => {
                    // got kill signal
                    break;
                }
                incoming_message = transport_incoming.next() => {
                    let do_exit = matches!(incoming_message, None | Some(Err(_)));
                    let incoming_message = incoming_message.map(|x| x.map_err(|e| match e {
                        Either::Left(e) => Error::IncomingError(e),
                        Either::Right(e) => Error::IRCParseError(e)
                    }));

                    if let Some(connection_loop_tx) = connection_loop_tx.upgrade() {
                        // unwrap(): We don't expect the connection loop to die before all tx clones
                        // are dropped (and we are holding one right now)
                        connection_loop_tx.send(ConnectionLoopCommand::IncomingMessage(incoming_message)).unwrap();
                    } else {
                        break;
                    }

                    if do_exit {
                        break;
                    }
                }
            }
        }
        log::debug!("Incoming messages forwarder ended");
    }

    async fn run_ping_task(
        connection_loop_tx: Weak<mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>>,
        mut shutdown_notify: oneshot::Receiver<()>,
    ) {
        log::debug!("Spawned pinger task");
        // every 30 seconds we send out a PING
        // 5 seconds after sending it out, we check that we got a PONG message since sending that PING
        // if not, the connection is failed with an error (Error::PingTimeout)
        let ping_every = Duration::from_secs(30);
        let check_pong_after = Duration::from_secs(5);

        let mut send_ping_interval = interval_at(Instant::now() + ping_every, ping_every);
        let mut check_pong_interval =
            interval_at(Instant::now() + ping_every + check_pong_after, ping_every);

        loop {
            tokio::select! {
                _ = &mut shutdown_notify => {
                    break;
                },
                _ = send_ping_interval.tick() => {
                    log::trace!("sending ping");
                    if let Some(connection_loop_tx) = connection_loop_tx.upgrade() {
                        connection_loop_tx.send(ConnectionLoopCommand::SendPing()).unwrap();
                    } else {
                        break;
                    }
                }
                _ = check_pong_interval.tick() => {
                    log::trace!("checking for pong");
                    if let Some(connection_loop_tx) = connection_loop_tx.upgrade() {
                        connection_loop_tx.send(ConnectionLoopCommand::CheckPong()).unwrap();
                    } else {
                        break;
                    }
                }
            }
        }
        log::debug!("Pinger task ended");
    }
}

impl<T: Transport, L: LoginCredentials> ConnectionLoopStateMethods<T, L>
    for ConnectionLoopInitializingState<T, L>
{
    fn send_message(
        &mut self,
        message: IRCMessage,
        reply_sender: Option<Sender<Result<(), Error<T, L>>>>,
    ) {
        self.commands_queue.push_back((message, reply_sender));
    }

    fn on_transport_init_finished(
        self,
        init_result: Result<(TransportStream<T>, CredentialsPair), Error<T, L>>,
    ) -> ConnectionLoopState<T, L> {
        match init_result {
            Ok((transport, credentials)) => {
                // transport was opened successfully
                log::debug!("Transport init task has finished, transitioning to Initializing");
                let (transport_incoming, transport_outgoing) = transport.split();

                let (kill_incoming_loop_tx, kill_incoming_loop_rx) = oneshot::channel();
                tokio::spawn(ConnectionLoopInitializingState::run_incoming_forward_task(
                    transport_incoming,
                    Weak::clone(&self.connection_loop_tx),
                    kill_incoming_loop_rx,
                ));

                let (kill_pinger_tx, kill_pinger_rx) = oneshot::channel();
                tokio::spawn(ConnectionLoopInitializingState::run_ping_task(
                    Weak::clone(&self.connection_loop_tx),
                    kill_pinger_rx,
                ));

                // transition our own state from Initializing to Open
                self.connection_incoming_tx
                    .send(ConnectionIncomingMessage::StateOpen)
                    .ok();

                let mut new_state = ConnectionLoopState::Open(ConnectionLoopOpenState {
                    transport_outgoing: Arc::new(Mutex::new(transport_outgoing)),
                    connection_loop_tx: self.connection_loop_tx,
                    connection_incoming_tx: self.connection_incoming_tx,
                    pong_received: false,
                    kill_incoming_loop_tx: Some(kill_incoming_loop_tx),
                    kill_pinger_tx: Some(kill_pinger_tx),
                });

                new_state.send_message(
                    irc!["CAP", "REQ", "twitch.tv/tags twitch.tv/commands"],
                    None,
                );
                if let Some(token) = credentials.token {
                    new_state.send_message(irc!["PASS", format!("oauth:{}", token)], None);
                }
                new_state.send_message(irc!["NICK", credentials.login], None);

                for (message, return_sender) in self.commands_queue.into_iter() {
                    new_state.send_message(message, return_sender);
                }

                new_state
            }
            Err(init_error) => {
                // emit error to downstream + transition to closed
                log::error!("Transport init task has finished with error, closing connection");
                self.transition_to_closed(Some(init_error))
            }
        }
    }

    fn on_send_error(self, error: <T as Transport>::OutgoingError) -> ConnectionLoopState<T, L> {
        self.transition_to_closed(Some(Error::OutgoingError(error)))
    }

    fn on_incoming_message(
        self,
        _maybe_message: Option<Result<IRCMessage, Error<T, L>>>,
    ) -> ConnectionLoopState<T, L> {
        unreachable!("messages cannot come in while initializing")
    }

    fn send_ping(&mut self) {
        unreachable!("pinger should not run while initializing")
    }

    fn check_pong(self) -> ConnectionLoopState<T, L> {
        unreachable!("pinger should not run while initializing")
    }
}

//
// OPEN STATE
//
struct ConnectionLoopOpenState<T: Transport, L: LoginCredentials> {
    transport_outgoing: Arc<Mutex<T::Outgoing>>,
    connection_loop_tx: Weak<mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>>,
    connection_incoming_tx: mpsc::UnboundedSender<ConnectionIncomingMessage<T, L>>,
    pong_received: bool,
    /// To kill the background pinger and forward tasks when this gets dropped.
    /// These fields are wrapped in `Option` so we can use `take()` in the Drop implementation.
    kill_incoming_loop_tx: Option<oneshot::Sender<()>>,
    kill_pinger_tx: Option<oneshot::Sender<()>>,
}

impl<T: Transport, L: LoginCredentials> ConnectionLoopOpenState<T, L> {
    fn transition_to_closed(self, cause: Option<Error<T, L>>) -> ConnectionLoopState<T, L> {
        log::info!("Closing connection, cause: {:?}", cause);

        let cause = cause.unwrap_or(Error::ConnectionClosed);

        self.connection_incoming_tx
            .send(ConnectionIncomingMessage::StateClosed { cause })
            .ok();

        // the shutdown notify is invoked via the Drop implementation

        // return the new state the connection should take on
        ConnectionLoopState::Closed(ConnectionLoopClosedState)
    }
}

impl<T: Transport, L: LoginCredentials> Drop for ConnectionLoopOpenState<T, L> {
    fn drop(&mut self) {
        self.kill_incoming_loop_tx.take().unwrap().send(()).ok();
        self.kill_pinger_tx.take().unwrap().send(()).ok();
    }
}

impl<T: Transport, L: LoginCredentials> ConnectionLoopStateMethods<T, L>
    for ConnectionLoopOpenState<T, L>
{
    fn send_message(
        &mut self,
        message: IRCMessage,
        reply_sender: Option<Sender<Result<(), Error<T, L>>>>,
    ) {
        let transport_outgoing = Arc::clone(&self.transport_outgoing);
        let connection_loop_tx = Weak::clone(&self.connection_loop_tx);
        tokio::spawn(async move {
            let mut transport_outgoing = transport_outgoing.lock().await;
            let res = transport_outgoing.send(message).await;

            // The error is cloned and sent both to the calling method as well as
            // the connection event loop so it can end with that error.
            if let Some(reply_sender) = reply_sender {
                reply_sender
                    .send(res.clone().map_err(Error::OutgoingError))
                    .ok();
            }
            if let Err(err) = res {
                if let Some(connection_loop_tx) = connection_loop_tx.upgrade() {
                    connection_loop_tx
                        .send(ConnectionLoopCommand::SendError(err))
                        .unwrap();
                    // unwrap: connection loop should not die before all of its senders
                    // are dropped.
                }
            }
        });
    }

    fn on_transport_init_finished(
        self,
        _init_result: Result<(TransportStream<T>, CredentialsPair), Error<T, L>>,
    ) -> ConnectionLoopState<T, L> {
        unreachable!("transport init cannot finish more than once")
    }

    fn on_send_error(self, error: <T as Transport>::OutgoingError) -> ConnectionLoopState<T, L> {
        self.transition_to_closed(Some(Error::OutgoingError(error)))
    }

    fn on_incoming_message(
        mut self,
        maybe_message: Option<Result<IRCMessage, Error<T, L>>>,
    ) -> ConnectionLoopState<T, L> {
        match maybe_message {
            None => {
                log::info!("EOF received from transport incoming stream");
                self.transition_to_closed(Some(Error::ConnectionClosed))
            }
            Some(Err(error)) => {
                log::error!("Error received from transport incoming stream: {}", error);
                self.transition_to_closed(Some(error))
            }
            Some(Ok(irc_message)) => {
                // Note! An error here (failing to parse to a ServerMessage) will not result
                // in a connection abort. This is by design. See for example
                // https://github.com/robotty/dank-twitch-irc/issues/22.
                // The message will just be ignored instead
                let server_message = ServerMessage::try_from(irc_message);

                match server_message {
                    Ok(server_message) => {
                        self.connection_incoming_tx
                            .send(ConnectionIncomingMessage::IncomingMessage(
                                server_message.clone(),
                            ))
                            .ok();

                        // handle message
                        // react to PING, PONG and RECONNECT
                        match &server_message {
                            ServerMessage::Ping(_) => {
                                self.send_message(irc!["PONG", "tmi.twitch.tv"], None);
                            }
                            ServerMessage::Pong(_) => {
                                log::trace!("Received pong");
                                self.pong_received = true;
                            }
                            ServerMessage::Reconnect(_) => {
                                // disconnect
                                return self.transition_to_closed(Some(Error::ReconnectCmd));
                            }
                            _ => {}
                        }
                    }
                    Err(parse_error) => {
                        log::error!("Failed to parse incoming message as ServerMessage (emitting as generic instead): {}", parse_error);
                        self.connection_incoming_tx
                            .send(ConnectionIncomingMessage::IncomingMessage(
                                ServerMessage::new_generic(IRCMessage::from(parse_error)),
                            ))
                            .ok();
                    }
                }

                // stay open
                ConnectionLoopState::Open(self)
            }
        }
    }

    fn send_ping(&mut self) {
        self.pong_received = false;
        self.send_message(irc!["PING", "tmi.twitch.tv"], None);
    }

    fn check_pong(self) -> ConnectionLoopState<T, L> {
        if !self.pong_received {
            // close down
            self.transition_to_closed(Some(Error::PingTimeout))
        } else {
            // stay open
            ConnectionLoopState::Open(self)
        }
    }
}

//
// CLOSED STATE.
//
struct ConnectionLoopClosedState;

impl<T: Transport, L: LoginCredentials> ConnectionLoopStateMethods<T, L>
    for ConnectionLoopClosedState
{
    fn send_message(
        &mut self,
        _message: IRCMessage,
        reply_sender: Option<Sender<Result<(), Error<T, L>>>>,
    ) {
        if let Some(reply_sender) = reply_sender {
            reply_sender.send(Err(Error::ConnectionClosed)).ok();
        }
    }

    fn on_transport_init_finished(
        self,
        _init_result: Result<(TransportStream<T>, CredentialsPair), Error<T, L>>,
    ) -> ConnectionLoopState<T, L> {
        // do nothing, stay closed
        ConnectionLoopState::Closed(self)
    }

    fn on_send_error(self, _error: T::OutgoingError) -> ConnectionLoopState<T, L> {
        // do nothing, stay closed
        ConnectionLoopState::Closed(self)
    }

    fn on_incoming_message(
        self,
        _maybe_message: Option<Result<IRCMessage, Error<T, L>>>,
    ) -> ConnectionLoopState<T, L> {
        // do nothing, stay closed
        ConnectionLoopState::Closed(self)
    }

    fn send_ping(&mut self) {}

    fn check_pong(self) -> ConnectionLoopState<T, L> {
        // do nothing, stay closed
        ConnectionLoopState::Closed(self)
    }
}
