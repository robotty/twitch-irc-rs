use crate::client::pool_connection::PoolConnection;
#[cfg(feature = "metrics-collection")]
use crate::client::pool_connection::ReportedConnectionState;
use crate::config::ClientConfig;
use crate::connection::event_loop::ConnectionLoopCommand;
use crate::connection::{Connection, ConnectionIncomingMessage};
use crate::error::Error;
use crate::irc;
use crate::login::LoginCredentials;
use crate::message::commands::ServerMessage;
use crate::message::{IRCMessage, JoinMessage, PartMessage};
#[cfg(feature = "metrics-collection")]
use crate::metrics::MetricsBundle;
use crate::transport::Transport;
use fast_str::FastStr;
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Weak};
use tokio::sync::{mpsc, oneshot};
use tracing::{info_span, Instrument};

#[derive(Debug)]
pub(crate) enum ClientLoopCommand<T: Transport, L: LoginCredentials> {
    Connect {
        return_sender: oneshot::Sender<()>,
    },
    SendMessage {
        message: IRCMessage,
        return_sender: oneshot::Sender<Result<(), Error<T, L>>>,
    },
    Join {
        channel_login: FastStr,
    },
    GetChannelStatus {
        channel_login: FastStr,
        return_sender: oneshot::Sender<(bool, bool)>,
    },
    Part {
        channel_login: FastStr,
    },
    SetWantedChannels {
        channels: HashSet<FastStr>,
    },
    Ping {
        return_sender: oneshot::Sender<Result<(), Error<T, L>>>,
    },
    IncomingMessage {
        source_connection_id: usize,
        message: Box<ConnectionIncomingMessage<T, L>>,
    },
}

pub(crate) struct ClientLoopWorker<T: Transport, L: LoginCredentials> {
    config: Arc<ClientConfig<L>>,
    next_connection_id: usize,
    /// the connection we currently forward WHISPER messages from. If we didn't do this,
    /// each WHISPER message would be received multiple times if we had more than
    /// one connection open.
    current_whisper_connection_id: Option<usize>,
    client_loop_rx: mpsc::UnboundedReceiver<ClientLoopCommand<T, L>>,
    connections: VecDeque<PoolConnection<T, L>>,
    client_loop_tx: Weak<mpsc::UnboundedSender<ClientLoopCommand<T, L>>>,
    client_incoming_messages_tx: mpsc::UnboundedSender<ServerMessage>,
    #[cfg(feature = "metrics-collection")]
    metrics: Option<MetricsBundle>,
}

impl<T: Transport, L: LoginCredentials> ClientLoopWorker<T, L> {
    pub fn spawn(
        config: Arc<ClientConfig<L>>,
        client_loop_tx: Weak<mpsc::UnboundedSender<ClientLoopCommand<T, L>>>,
        client_loop_rx: mpsc::UnboundedReceiver<ClientLoopCommand<T, L>>,
        client_incoming_messages_tx: mpsc::UnboundedSender<ServerMessage>,
        #[cfg(feature = "metrics-collection")] metrics: Option<MetricsBundle>,
    ) {
        let span = match &config.tracing_identifier {
            Some(s) => info_span!("client_loop", name = %s),
            None => info_span!("client_loop"),
        };

        let worker = ClientLoopWorker {
            config,
            next_connection_id: 0,
            current_whisper_connection_id: None,
            client_loop_rx,
            connections: VecDeque::new(),
            client_loop_tx,
            client_incoming_messages_tx,
            #[cfg(feature = "metrics-collection")]
            metrics,
        };

        tokio::spawn(worker.run().instrument(span));
    }

    async fn run(mut self) {
        tracing::debug!("Spawned client event loop");
        while let Some(command) = self.client_loop_rx.recv().await {
            self.process_command(command);
        }
        tracing::debug!("Client event loop ended")
    }

    fn process_command(&mut self, command: ClientLoopCommand<T, L>) {
        match command {
            ClientLoopCommand::Connect { return_sender } => {
                if self.connections.is_empty() {
                    let new_connection = self.make_new_connection();
                    self.connections.push_back(new_connection);
                    self.update_metrics();
                }
                return_sender.send(()).ok();
            }
            ClientLoopCommand::SendMessage {
                message,
                return_sender,
            } => self.send_message(message, return_sender),
            ClientLoopCommand::Join { channel_login } => self.join(channel_login),
            ClientLoopCommand::SetWantedChannels { channels } => self.set_wanted_channels(channels),
            ClientLoopCommand::GetChannelStatus {
                channel_login,
                return_sender,
            } => {
                return_sender
                    .send(self.get_channel_status(channel_login))
                    .ok();
            }
            ClientLoopCommand::Part { channel_login } => self.part(channel_login),
            ClientLoopCommand::Ping { return_sender } => self.ping(return_sender),
            ClientLoopCommand::IncomingMessage {
                source_connection_id,
                message,
            } => self.on_incoming_message(source_connection_id, *message),
        }
    }

    #[must_use]
    fn make_new_connection(&mut self) -> PoolConnection<T, L> {
        let connection_id = self.next_connection_id;
        // .0 at the end: the overflowing_add method returns a tuple (u64, bool)
        // with the resulting value and whether an overflow occurred. we ignore the bool and just
        // take the value.
        self.next_connection_id = self.next_connection_id.overflowing_add(1).0;

        tracing::info!("Making a new pool connection, new ID is {}", connection_id);

        let (connection_incoming_messages_rx, connection) = Connection::new(
            Arc::clone(&self.config),
            connection_id,
            #[cfg(feature = "metrics-collection")]
            self.metrics.clone(),
        );
        let (tx_kill_incoming, rx_kill_incoming) = oneshot::channel();

        let pool_conn = PoolConnection::new(
            Arc::clone(&self.config),
            connection_id,
            connection,
            tx_kill_incoming,
        );

        // forward messages.
        tokio::spawn(
            ClientLoopWorker::run_incoming_forward_task(
                connection_incoming_messages_rx,
                connection_id,
                self.client_loop_tx.clone(),
                rx_kill_incoming,
            )
            .instrument(info_span!("incoming_forward_task", connection_id)),
        );

        pool_conn
    }

    /// forwards messages from a Connection to the client event loop.
    async fn run_incoming_forward_task(
        mut connection_incoming_messages_rx: mpsc::UnboundedReceiver<
            ConnectionIncomingMessage<T, L>,
        >,
        connection_id: usize,
        client_loop_tx: Weak<mpsc::UnboundedSender<ClientLoopCommand<T, L>>>,
        mut rx_kill_incoming: oneshot::Receiver<()>,
    ) {
        loop {
            // todo add tracing calls
            tokio::select! {
                _ = &mut rx_kill_incoming => {
                    break;
                }
                incoming_message = connection_incoming_messages_rx.recv() => {
                    if let Some(incoming_message) = incoming_message {
                        if let Some(client_loop_tx) = client_loop_tx.upgrade() {
                            client_loop_tx.send(ClientLoopCommand::IncomingMessage {
                                source_connection_id: connection_id,
                                message: Box::new(incoming_message)
                            }).unwrap();
                        } else {
                            // all TwitchIRCClient handles have been dropped, so all background
                            // tasks are implicitly terminated too.
                            break;
                        }
                    } else {
                        // end of stream coming from connection
                        break;
                    }
                }
            }
        }
    }

    fn send_message(
        &mut self,
        message: IRCMessage,
        return_sender: oneshot::Sender<Result<(), Error<T, L>>>,
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

        pool_connection
            .connection
            .connection_loop_tx
            .send(ConnectionLoopCommand::SendMessage(
                message,
                Some(return_sender),
            ))
            .unwrap();

        // put the connection back to the end of the queue
        self.connections.push_back(pool_connection);

        // count up created connections counter
        #[cfg(feature = "metrics-collection")]
        if let Some(ref metrics) = self.metrics {
            metrics.connections_created.inc();
        }

        self.update_metrics();
    }

    /// Instructs the client to now start "wanting to be joined" to that channel.
    ///
    /// The client will make best attempts to stay joined to this channel. I/O errors will be
    /// compensated by retrying the join process. For this reason, this method returns no error.
    fn join(&mut self, channel_login: FastStr) {
        let channel_already_confirmed_joined = self.connections.iter().any(|c| {
            c.wanted_channels.contains(&channel_login) && c.server_channels.contains(&channel_login)
        });

        // skip the join altogether if we are already confirmed to be joined to that channel.
        if channel_already_confirmed_joined {
            return;
        }

        let mut pool_connection = self
            .connections
            .iter()
            // has any of the connections already previously tried to join this channel? then we pick that one.
            .position(|c| c.wanted_channels.contains(&channel_login))
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
            // or else make a new connection
            .unwrap_or_else(|| self.make_new_connection());

        // delegate join command to connection
        pool_connection
            .connection
            .connection_loop_tx
            .send(ConnectionLoopCommand::SendMessage(
                irc!["JOIN", format!("#{}", channel_login)],
                None,
            ))
            .unwrap();

        pool_connection.register_sent_message();
        pool_connection.wanted_channels.insert(channel_login);

        // put the connection back to the end of the queue
        self.connections.push_back(pool_connection);
        // update metrics about channel numbers
        self.update_metrics();
    }

    fn set_wanted_channels(&mut self, channels: HashSet<FastStr>) {
        // part channels as needed
        self.connections
            .iter()
            .flat_map(|conn| conn.wanted_channels.difference(&channels))
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .for_each(|channel_login| self.part(channel_login));

        // join all wanted channels. Channels already joined will be detected
        // inside the join method.
        for channel_login in channels {
            self.join(channel_login);
        }
    }

    fn get_channel_status(&mut self, channel_login: FastStr) -> (bool, bool) {
        let wanted = self
            .connections
            .iter()
            .any(|c| c.wanted_channels.contains(&channel_login));
        let joined_on_server = self
            .connections
            .iter()
            .any(|c| c.server_channels.contains(&channel_login));
        (wanted, joined_on_server)
    }

    fn part(&mut self, channel_login: FastStr) {
        // skip the PART altogether if the last message we sent regarding that channel was a PART
        // (or nothing at all, for that matter).
        if self
            .connections
            .iter()
            .all(|c| !c.wanted_channels.contains(&channel_login))
        {
            return;
        }

        // now grab the connection that has that channel
        let mut pool_connection = self
            .connections
            .iter()
            .position(|c| c.wanted_channels.contains(&channel_login))
            .and_then(|pos| self.connections.remove(pos))
            .unwrap();

        // delegate part command to connection
        pool_connection
            .connection
            .connection_loop_tx
            .send(ConnectionLoopCommand::SendMessage(
                irc!["PART", format!("#{}", channel_login)],
                None,
            ))
            .unwrap();

        pool_connection.register_sent_message();
        pool_connection.wanted_channels.remove(&channel_login);

        // put the connection back to the end of the queue
        self.connections.push_back(pool_connection);
        // update metrics about channel numbers
        self.update_metrics();
    }

    fn ping(&mut self, return_sender: oneshot::Sender<Result<(), Error<T, L>>>) {
        self.send_message(irc!["PING", "tmi.twitch.tv"], return_sender)
    }

    fn on_incoming_message(
        &mut self,
        source_connection_id: usize,
        message: ConnectionIncomingMessage<T, L>,
    ) {
        match message {
            ConnectionIncomingMessage::IncomingMessage(message) => {
                let is_whisper = matches!(*message, ServerMessage::Whisper(_));
                if is_whisper {
                    match self.current_whisper_connection_id {
                        Some(current_whisper_connection_id) => {
                            // another connection is already the chosen connection for whispers
                            // so we ignore this message if it doesn't come from that connection
                            if current_whisper_connection_id != source_connection_id {
                                tracing::debug!(
                                    "Ignoring whisper from connection {} (not whisper connection)",
                                    source_connection_id
                                );
                                return; // ignore message, don't forward.
                            }
                            tracing::debug!("Received whisper from connection {}, will be forwarded as it is the current whisper connection", source_connection_id)
                        }
                        None => {
                            // no connection chosen to be whisper connection yet
                            // since we just got a whisper, we will assign this connection to
                            // now be the responsible whisper connection. (and the message
                            // will be forwarded)
                            tracing::debug!("Received whisper and had no whisper connection selected. Selecting pool connection {}. Message was forwarded", source_connection_id);
                            self.current_whisper_connection_id = Some(source_connection_id)
                        }
                    }
                }

                match &*message {
                    ServerMessage::Join(JoinMessage { channel_login, .. }) => {
                        // we successfully joined a channel
                        let c = self
                            .connections
                            .iter_mut()
                            .find(|c| c.id == source_connection_id)
                            .unwrap();
                        c.server_channels.insert(channel_login.clone());

                        // update metrics about channel numbers
                        self.update_metrics();
                    }
                    ServerMessage::Part(PartMessage { channel_login, .. }) => {
                        // we successfully parted a channel
                        let c = self
                            .connections
                            .iter_mut()
                            .find(|c| c.id == source_connection_id)
                            .unwrap();
                        let channel_login = FastStr::from_ref(channel_login);
                        c.server_channels.remove::<FastStr>(&channel_login);

                        // update metrics about channel numbers
                        self.update_metrics();
                    }
                    _ => {}
                }

                self.client_incoming_messages_tx.send(*message).ok(); // ignore if the library user is not using the incoming messages
            }
            #[cfg(feature = "metrics-collection")]
            ConnectionIncomingMessage::StateOpen => {
                let c = self
                    .connections
                    .iter_mut()
                    .find(|c| c.id == source_connection_id)
                    .unwrap();
                c.reported_state = ReportedConnectionState::Open;
                self.update_metrics();
            }
            ConnectionIncomingMessage::StateClosed { cause } => {
                tracing::error!(
                    "Pool connection {} has failed due to error (removing it): {}",
                    source_connection_id,
                    cause
                );

                // remove it from the list of connections.
                // unwrap(): asserts that this is the first and only time we get an Err from
                // that connection
                let mut pool_connection = self
                    .connections
                    .iter()
                    .position(|c| c.id == source_connection_id)
                    .and_then(|pos| self.connections.remove(pos))
                    .unwrap();

                // count up failed connections counter
                #[cfg(feature = "metrics-collection")]
                if let Some(ref metrics) = self.metrics {
                    metrics.connections_failed.inc();
                }
                // also update twitch_irc_channels and twitch_irc_connections gauges
                self.update_metrics();

                // rejoin channels
                tracing::debug!(
                    "Pool connection {} previously was joined to {} channels ({:?}), rejoining them",
                    source_connection_id,
                    pool_connection.wanted_channels.len(),
                    pool_connection.wanted_channels
                );
                for channel in pool_connection.wanted_channels.drain() {
                    self.join(channel);
                }

                // remove it from role of "current whisper connection" if it was whisper conn before
                if self.current_whisper_connection_id == Some(source_connection_id) {
                    tracing::debug!(
                        "Connection {} was whisper connection, removing it",
                        source_connection_id
                    );
                    self.current_whisper_connection_id = None;
                }

                // make sure we stay connected in order to receive whispers
                if self.connections.is_empty() {
                    let new_connection = self.make_new_connection();
                    self.connections.push_back(new_connection);
                    self.update_metrics();
                }
            }
        }
    }

    #[cfg(feature = "metrics-collection")]
    fn update_metrics(&mut self) {
        if let Some(ref metrics) = self.metrics {
            let (num_initializing, num_open) = self
                .connections
                .iter()
                .map(|c| match &c.reported_state {
                    ReportedConnectionState::Initializing => (1i64, 0i64),
                    ReportedConnectionState::Open => (0i64, 1i64),
                })
                // sum up all the tuples (like vectors)
                .fold((0i64, 0i64), |(a, b), (c, d)| (a + c, b + d));

            metrics
                .connections
                .with_label_values(&["initializing"])
                .set(num_initializing);
            metrics
                .connections
                .with_label_values(&["open"])
                .set(num_open);

            let (num_wanted, num_server) = self
                .connections
                .iter()
                .map(|c| {
                    (
                        c.wanted_channels.len() as i64,
                        c.server_channels.len() as i64,
                    )
                })
                // sum up all the tuples (like vectors)
                .fold((0, 0), |(a, b), (c, d)| (a + c, b + d));

            metrics
                .channels
                .with_label_values(&["wanted"])
                .set(num_wanted);
            metrics
                .channels
                .with_label_values(&["server"])
                .set(num_server);
        }
    }

    #[cfg(not(feature = "metrics-collection"))]
    fn update_metrics(&mut self) {}
}
