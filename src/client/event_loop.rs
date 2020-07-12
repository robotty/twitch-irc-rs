use crate::client::pool_connection::PoolConnection;
use crate::config::ClientConfig;
use crate::connection::error::ConnectionError;
use crate::connection::Connection;
use crate::irc;
use crate::login::LoginCredentials;
use crate::message::commands::{AsIRCMessage, ServerMessage};
use crate::message::IRCMessage;
use crate::transport::Transport;
use enum_dispatch::enum_dispatch;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures::stream::Next;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::task::JoinHandle;

#[derive(Debug)]
pub(crate) enum ClientLoopCommand<T: Transport, L: LoginCredentials> {
    Connect {
        return_sender: oneshot::Sender<()>,
    },
    SendMessage {
        message: IRCMessage,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    },
    Privmsg {
        channel_login: String,
        message: String,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    },
    Join {
        channel_login: String,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    },
    Part {
        channel_login: String,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    },
    Ping {
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    },
    Close {
        return_sender: Option<oneshot::Sender<()>>,
    },
    IncomingMessage {
        source_connection_id: usize,
        message: Option<Result<ServerMessage, (ConnectionError<T, L>, HashSet<String>)>>,
    },
}

#[enum_dispatch]
trait ClientLoopStateImpl<T: Transport, L: LoginCredentials> {
    fn next_command(
        &mut self,
    ) -> futures::stream::Next<mpsc::UnboundedReceiver<ClientLoopCommand<T, L>>>;
    fn connect(&mut self);
    fn send_message(
        &mut self,
        message: IRCMessage,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    );
    fn privmsg(
        &mut self,
        channel_login: String,
        message: String,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    );
    fn join(
        &mut self,
        channel_login: String,
        return_sender: Option<oneshot::Sender<Result<(), ConnectionError<T, L>>>>,
    );
    fn part(
        &mut self,
        channel_login: String,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    );
    fn ping(&mut self, return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>);
    fn on_incoming_message(
        &mut self,
        source_connection_id: usize,
        message: Option<Result<ServerMessage, (ConnectionError<T, L>, HashSet<String>)>>,
    );
}

#[enum_dispatch(ClientLoopStateImpl)]
pub(crate) enum ClientLoopWorker<T: Transport, L: LoginCredentials> {
    Open(ClientLoopOpenState<T, L>),
    Closed(ClientLoopClosedState<T, L>),
}

impl<T: Transport, L: LoginCredentials> ClientLoopWorker<T, L> {
    pub fn new(
        config: Arc<ClientConfig<L>>,
        client_loop_tx: mpsc::UnboundedSender<ClientLoopCommand<T, L>>,
        client_loop_rx: mpsc::UnboundedReceiver<ClientLoopCommand<T, L>>,
        client_incoming_messages_tx: mpsc::UnboundedSender<ServerMessage>,
    ) -> ClientLoopWorker<T, L> {
        ClientLoopWorker::Open(ClientLoopOpenState {
            config,
            next_connection_id: 0,
            current_whisper_connection_id: None,
            client_loop_rx,
            connections: VecDeque::new(),
            client_loop_tx,
            client_incoming_messages_tx,
        })
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        log::debug!("Spawned client event loop");
        while let Some(command) = self.next_command().await {
            self = self.process_command(command);
        }
        log::debug!("Client event loop ended")
    }

    fn process_command(mut self, command: ClientLoopCommand<T, L>) -> Self {
        match command {
            ClientLoopCommand::Connect { return_sender } => {
                self.connect();
                return_sender.send(()).ok();
            }
            ClientLoopCommand::SendMessage {
                message,
                return_sender,
            } => self.send_message(message, return_sender),
            ClientLoopCommand::Privmsg {
                channel_login,
                message,
                return_sender,
            } => self.privmsg(channel_login, message, return_sender),
            ClientLoopCommand::Join {
                channel_login,
                return_sender,
            } => self.join(channel_login, Some(return_sender)),
            ClientLoopCommand::Part {
                channel_login,
                return_sender,
            } => self.part(channel_login, return_sender),
            ClientLoopCommand::Ping { return_sender } => self.ping(return_sender),
            ClientLoopCommand::Close { return_sender } => {
                self = self.close();
                if let Some(return_sender) = return_sender {
                    return_sender.send(()).ok();
                }
            }
            ClientLoopCommand::IncomingMessage {
                source_connection_id,
                message,
            } => self.on_incoming_message(source_connection_id, message),
        }
        self
    }

    fn close(mut self) -> Self {
        match self {
            ClientLoopWorker::Open(ClientLoopOpenState { client_loop_rx, .. }) => {
                self = ClientLoopWorker::Closed(ClientLoopClosedState { client_loop_rx })
            }
            ClientLoopWorker::Closed(_) => {}
        }
        self
    }
}

pub(crate) struct ClientLoopOpenState<T: Transport, L: LoginCredentials> {
    config: Arc<ClientConfig<L>>,
    next_connection_id: usize,
    /// the connection we currently forward WHISPER messages from. If we didn't do this,
    /// each WHISPER message would be received multiple times if we had more than
    /// one connection open.
    current_whisper_connection_id: Option<usize>,
    client_loop_rx: mpsc::UnboundedReceiver<ClientLoopCommand<T, L>>,
    connections: VecDeque<PoolConnection<T, L>>,
    client_loop_tx: mpsc::UnboundedSender<ClientLoopCommand<T, L>>,
    client_incoming_messages_tx: mpsc::UnboundedSender<ServerMessage>,
}

impl<T: Transport, L: LoginCredentials> ClientLoopOpenState<T, L> {
    fn make_new_connection(&mut self) -> PoolConnection<T, L> {
        let mut connection = Connection::new(Arc::clone(&self.config));
        let (tx_kill_incoming, rx_kill_incoming) = oneshot::channel();
        let connection_incoming_messages_rx = connection.incoming_messages.take().unwrap();

        let connection_id = self.next_connection_id;
        // .0 at the end: the overflowing_add method returns a tuple (u64, bool)
        // with the resulting value and whether an overflow occurred. we ignore the bool and just
        // take the value.
        self.next_connection_id = self.next_connection_id.overflowing_add(1).0;

        log::info!("Making a new pool connection, new ID is {}", connection_id);

        let pool_conn = PoolConnection::new(
            Arc::clone(&self.config),
            connection_id,
            connection,
            tx_kill_incoming,
        );

        // forward messages.
        ClientLoopOpenState::spawn_incoming_forward_task(
            connection_incoming_messages_rx,
            connection_id,
            self.client_loop_tx.clone(),
            rx_kill_incoming,
        );

        pool_conn
    }

    /// forwards messages from a Connection to the client event loop.
    fn spawn_incoming_forward_task(
        mut connection_incoming_messages_rx: mpsc::UnboundedReceiver<
            Result<ServerMessage, (ConnectionError<T, L>, HashSet<String>)>,
        >,
        connection_id: usize,
        client_loop_tx: mpsc::UnboundedSender<ClientLoopCommand<T, L>>,
        mut rx_kill_incoming: oneshot::Receiver<()>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut rx_kill_incoming => {
                        break;
                    }
                    incoming_message = connection_incoming_messages_rx.next() => {
                        let is_end_of_stream = incoming_message.is_none();

                        client_loop_tx.unbounded_send(ClientLoopCommand::IncomingMessage {
                            source_connection_id: connection_id,
                            message: incoming_message
                        }).unwrap();

                        if is_end_of_stream {
                            break;
                        }
                    }
                }
            }
        })
    }
}

impl<T: Transport, L: LoginCredentials> ClientLoopStateImpl<T, L> for ClientLoopOpenState<T, L> {
    fn next_command(&mut self) -> Next<mpsc::UnboundedReceiver<ClientLoopCommand<T, L>>> {
        self.client_loop_rx.next()
    }

    fn connect(&mut self) {
        if self.connections.is_empty() {
            self.make_new_connection();
        }
    }

    fn send_message(
        &mut self,
        message: IRCMessage,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    ) {
        let mut pool_connection = self
            .connections
            .iter()
            .position(|c| c.not_busy())
            // take what we found
            .map(|pos| self.connections.remove(pos).unwrap())
            // or else make a new one
            .unwrap_or_else(|| self.make_new_connection());

        pool_connection.register_sent_message();

        // make a clone of the inner Connection struct, and then send out the message asynchronously
        let connection = Arc::clone(&pool_connection.connection);
        tokio::spawn(async move {
            let res = connection.send_message(message).await;
            return_sender.send(res).ok();
        });

        // put the connection back to the end of the queue
        self.connections.push_back(pool_connection);
    }

    fn privmsg(
        &mut self,
        channel_login: String,
        message: String,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    ) {
        // TODO apply the twitch-imposed rate limiting here
        self.send_message(
            irc!["PRIVMSG", format!("#{}", channel_login), message],
            return_sender,
        )
    }

    fn join(
        &mut self,
        channel_login: String,
        return_sender: Option<oneshot::Sender<Result<(), ConnectionError<T, L>>>>,
    ) {
        let mut pool_connection = self
            .connections
            .iter()
            // has any of the connections already joined this channel? then we pick that one.
            .position(|c| c.allocated_channels.contains(&channel_login))
            // if not, pick one that has not reached the channel limit.
            // Note we don't check "not busy" here
            // (to save on lots of connections being created when many channels are requested at once)
            .or_else(|| {
                self.connections
                    .iter()
                    .position(|c| c.channels_limit_not_reached())
            })
            // take what we found
            .map(|pos| self.connections.remove(pos).unwrap())
            // or else make a new one
            .unwrap_or_else(|| self.make_new_connection());

        pool_connection.register_sent_message();
        pool_connection
            .allocated_channels
            .insert(channel_login.clone());

        // make a clone of the inner Connection struct, and then send out the message asynchronously
        let connection = Arc::clone(&pool_connection.connection);
        tokio::spawn(async move {
            let res = connection.join(channel_login).await;
            if let Some(return_sender) = return_sender {
                return_sender.send(res).ok();
            }
        });

        // put the connection back to the end of the queue
        self.connections.push_back(pool_connection);
    }

    fn part(
        &mut self,
        channel_login: String,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    ) {
        let pool_connection = self
            .connections
            .iter()
            // has any of the connections joined this channel? then we pick that one.
            // if not then there is nothing to do
            .position(|c| c.allocated_channels.contains(&channel_login))
            // take what we found
            .map(|pos| self.connections.remove(pos).unwrap());

        // if there is nothing to do we return Ok(()) immediately and then return
        let mut pool_connection = match pool_connection {
            Some(pool_connection) => pool_connection,
            None => {
                return_sender.send(Ok(())).ok();
                return;
            }
        };

        pool_connection.register_sent_message();
        pool_connection.allocated_channels.remove(&channel_login);

        // make a clone of the inner Connection struct, and then send out the message asynchronously
        let connection = Arc::clone(&pool_connection.connection);
        tokio::spawn(async move {
            let res = connection.part(channel_login).await;
            return_sender.send(res).ok();
        });

        // put the connection back to the end of the queue
        self.connections.push_back(pool_connection);
    }

    fn ping(&mut self, return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>) {
        self.send_message(irc!["PING", "tmi.twitch.tv"], return_sender)
    }

    fn on_incoming_message(
        &mut self,
        source_connection_id: usize,
        message: Option<Result<ServerMessage, (ConnectionError<T, L>, HashSet<String>)>>,
    ) {
        match message {
            Some(Ok(message)) => {
                // TODO add a Whisper type, then use matches! here
                let is_whisper = message.as_irc_message().command == "WHISPER";

                if is_whisper {
                    match self.current_whisper_connection_id {
                        Some(current_whisper_connection_id) => {
                            // another connection is already the chosen connection for whispers
                            // so we ignore this message if it doesn't come from that connection
                            if current_whisper_connection_id != source_connection_id {
                                log::debug!(
                                    "Ignoring whisper from connection {} (not whisper connection)",
                                    source_connection_id
                                );
                                return; // ignore message, don't forward.
                            }
                            log::debug!("Received whisper from connection {}, will be forwarded as it is the current whisper connection", source_connection_id)
                        }
                        None => {
                            // no connection chosen to be whisper connection yet
                            // since we just got a whisper, we will assign this connection to
                            // now be the responsible whisper connection. (and the message
                            // will be forwarded)
                            log::debug!("Received whisper and had no whisper connection selected. Selecting pool connection {}. Message was forwarded", source_connection_id);
                            self.current_whisper_connection_id = Some(source_connection_id)
                        }
                    }
                }

                self.client_incoming_messages_tx
                    .unbounded_send(message)
                    .ok(); // ignore if the library user is not using the incoming messages
            }
            Some(Err((err, channels))) => {
                log::debug!(
                    "Received error from connection {}: {:?}",
                    source_connection_id,
                    err
                );

                // remove it from the list of connections.
                // unwrap(): asserts that this is the first and only time we get an Err from
                // that connection

                log::error!(
                    "Pool connection {} has failed, removing it",
                    source_connection_id
                );
                let position = self
                    .connections
                    .iter()
                    .position(|c| c.id == source_connection_id)
                    .unwrap();
                self.connections.remove(position).unwrap();

                // rejoin channels
                log::debug!(
                    "Pool connection {} previously was joined to {} channels {:?}, rejoining them",
                    source_connection_id,
                    channels.len(),
                    channels
                );
                for channel in channels.into_iter() {
                    self.join(channel, None);
                }

                // remove it from role of "current whisper connection" if it was whisper conn before
                if self.current_whisper_connection_id == Some(source_connection_id) {
                    log::debug!(
                        "Connection {} was whisper connection, removing it",
                        source_connection_id
                    );
                    self.current_whisper_connection_id = None;
                }

                // make sure we stay connected, this will make a new connection if there are now
                // 0 connections
                self.connect();
            }
            None => {
                // connection will always send an Err before sending None (End of Stream)
                // assert that this connection has been removed already
                assert!(self
                    .connections
                    .iter()
                    .all(|c| c.id != source_connection_id))
            }
        }
    }
}

pub(crate) struct ClientLoopClosedState<T: Transport, L: LoginCredentials> {
    client_loop_rx: mpsc::UnboundedReceiver<ClientLoopCommand<T, L>>,
}

impl<T: Transport, L: LoginCredentials> ClientLoopStateImpl<T, L> for ClientLoopClosedState<T, L> {
    fn next_command(&mut self) -> Next<mpsc::UnboundedReceiver<ClientLoopCommand<T, L>>> {
        self.client_loop_rx.next()
    }

    fn connect(&mut self) {
        panic!("invalid state")
    }

    fn send_message(
        &mut self,
        _message: IRCMessage,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    ) {
        return_sender.send(Err(ConnectionError::ClientClosed)).ok();
    }

    fn privmsg(
        &mut self,
        _channel_login: String,
        _message: String,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    ) {
        return_sender.send(Err(ConnectionError::ClientClosed)).ok();
    }

    fn join(
        &mut self,
        _channel_login: String,
        return_sender: Option<oneshot::Sender<Result<(), ConnectionError<T, L>>>>,
    ) {
        if let Some(return_sender) = return_sender {
            return_sender.send(Err(ConnectionError::ClientClosed)).ok();
        }
    }

    fn part(
        &mut self,
        _channel_login: String,
        return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>,
    ) {
        return_sender.send(Err(ConnectionError::ClientClosed)).ok();
    }

    fn ping(&mut self, return_sender: oneshot::Sender<Result<(), ConnectionError<T, L>>>) {
        return_sender.send(Err(ConnectionError::ClientClosed)).ok();
    }

    fn on_incoming_message(
        &mut self,
        _source_connection_id: usize,
        _message: Option<Result<ServerMessage, (ConnectionError<T, L>, HashSet<String>)>>,
    ) {
        // message is ignored
    }
}
