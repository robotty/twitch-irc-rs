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
use smallvec::SmallVec;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{Arc, Weak};
use tokio::sync::{mpsc, oneshot};
use tracing::{Instrument, info_span};

#[derive(Debug, Default)]
struct ChannelState {
    /// Connection ID that currently wants this channel (at most one).
    wanted_on: Option<usize>,
    /// Connection ID(s) that currently have server-confirmed `JOIN` for this channel.
    /// Usually 0 or 1; can transiently hold >1 if parted on one connection and re-joined on
    /// another before the server echoes `PART`.
    server_on: SmallVec<[usize; 1]>,
}

/// Borrows or reuses an existing `Arc<str>` channel key so confirmed-channel lookups skip
/// allocation and `StateClosed` rejoins reuse existing `Arc<str>` keys directly.
enum ChannelKey {
    Owned(String),
    Shared(Arc<str>),
}

impl ChannelKey {
    /// Returns the channel login as a string slice without allocating.
    fn as_str(&self) -> &str {
        match self {
            ChannelKey::Owned(s) => s.as_str(),
            ChannelKey::Shared(s) => s.as_ref(),
        }
    }

    /// Converts into an `Arc<str>` key for insertion into `channel_index` / `wanted_channels`.
    fn into_arc(self) -> Arc<str> {
        match self {
            ChannelKey::Owned(s) => Arc::from(s.as_str()),
            ChannelKey::Shared(s) => s,
        }
    }
}

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
        channel_login: String,
    },
    GetChannelStatus {
        channel_login: String,
        return_sender: oneshot::Sender<(bool, bool)>,
    },
    Part {
        channel_login: String,
    },
    SetWantedChannels {
        channels: HashSet<String>,
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
    /// Active pool connections keyed by connection ID for O(1) lookup.
    connections: HashMap<usize, PoolConnection<T, L>>,
    /// Round-robin queue of active connection IDs used when picking a non-busy connection
    /// for outgoing messages (`send_message`). Contains each live connection ID exactly once.
    send_order: VecDeque<usize>,
    /// Queue of connection IDs that have not yet reached `max_channels_per_connection`.
    /// Entries are pushed when a connection is created with capacity or drops below the limit
    /// after a `part()`, and popped/re-checked on `join()`.
    non_full_connections: VecDeque<usize>,
    /// Central O(1) channel index mapping each wanted or server-joined channel login (shared via
    /// `Arc<str>` with `PoolConnection` sets) to its current wanted and server-confirmed state.
    channel_index: HashMap<Arc<str>, ChannelState>,
    /// Running total of wanted channels across all active connections (for O(1) metric updates).
    #[cfg(feature = "metrics-collection")]
    num_wanted_channels: usize,
    /// Running total of server-confirmed channels across all active connections (for O(1) metric updates).
    #[cfg(feature = "metrics-collection")]
    num_server_channels: usize,
    /// Running count of connections in the `Initializing` state (for O(1) metric updates).
    #[cfg(feature = "metrics-collection")]
    num_initializing_connections: usize,
    /// Running count of connections in the `Open` state (for O(1) metric updates).
    #[cfg(feature = "metrics-collection")]
    num_open_connections: usize,
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
        let span = if let Some(s) = &config.tracing_identifier {
            info_span!("client_loop", name = %s)
        } else {
            info_span!("client_loop")
        };

        let worker = ClientLoopWorker {
            config,
            next_connection_id: 0,
            current_whisper_connection_id: None,
            client_loop_rx,
            connections: HashMap::new(),
            send_order: VecDeque::new(),
            non_full_connections: VecDeque::new(),
            channel_index: HashMap::new(),
            #[cfg(feature = "metrics-collection")]
            num_wanted_channels: 0,
            #[cfg(feature = "metrics-collection")]
            num_server_channels: 0,
            #[cfg(feature = "metrics-collection")]
            num_initializing_connections: 0,
            #[cfg(feature = "metrics-collection")]
            num_open_connections: 0,
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
        tracing::debug!("Client event loop ended");
    }

    fn process_command(&mut self, command: ClientLoopCommand<T, L>) {
        match command {
            ClientLoopCommand::Connect { return_sender } => {
                if self.connections.is_empty() {
                    self.insert_new_connection(true);
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

    /// Creates a new pool connection, registers it in `connections`, `send_order`, and the
    /// initializing-connection metric, and returns its ID. When `add_to_non_full` is `true` and
    /// the connection has channel capacity, its ID is also enqueued in `non_full_connections`.
    fn insert_new_connection(&mut self, add_to_non_full: bool) -> usize {
        let new_connection = self.make_new_connection();
        let connection_id = new_connection.id;
        let has_room = new_connection.channels_limit_not_reached();

        self.connections.insert(connection_id, new_connection);
        self.send_order.push_back(connection_id);
        if add_to_non_full && has_room {
            self.non_full_connections.push_back(connection_id);
        }

        #[cfg(feature = "metrics-collection")]
        {
            self.num_initializing_connections += 1;
        }

        connection_id
    }

    /// Pops the next live connection ID from `non_full_connections` that still has room under
    /// `max_channels_per_connection`, discarding any stale or already-full entries.
    fn pop_non_full_connection_id(&mut self) -> Option<usize> {
        while let Some(cid) = self.non_full_connections.pop_front() {
            if self
                .connections
                .get(&cid)
                .is_some_and(PoolConnection::channels_limit_not_reached)
            {
                return Some(cid);
            }
        }
        None
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
        let maybe_cid = self
            .send_order
            .iter()
            .position(|id| {
                self.connections
                    .get(id)
                    .is_some_and(PoolConnection::not_busy)
            })
            .map(|pos| self.send_order.remove(pos).unwrap());

        let cid = match maybe_cid {
            Some(cid) => {
                // put the connection back to the end of the queue
                self.send_order.push_back(cid);
                cid
            }
            None => self.insert_new_connection(true),
        };

        let pool_connection = self.connections.get_mut(&cid).unwrap();
        pool_connection.register_sent_message();

        pool_connection
            .connection
            .connection_loop_tx
            .send(ConnectionLoopCommand::SendMessage(
                message,
                Some(return_sender),
            ))
            .unwrap();

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
    fn join(&mut self, channel_login: String) {
        self.join_inner(ChannelKey::Owned(channel_login), true);
    }

    /// Joins (or retries an unconfirmed join for) `channel_key` via `channel_index` in O(1).
    /// Batch callers (`set_wanted_channels`, `StateClosed`) pass `update_metrics = false` and
    /// flush metrics once after the loop.
    fn join_inner(&mut self, channel_key: ChannelKey, update_metrics: bool) {
        if let Some(state) = self.channel_index.get(channel_key.as_str())
            && let Some(cid) = state.wanted_on
        {
            // skip the join altogether if we are already confirmed to be joined to that channel.
            if state.server_on.contains(&cid) {
                return;
            }

            // previously attempted on `cid` but not yet confirmed: retry JOIN on that connection.
            let pool_connection = self.connections.get_mut(&cid).unwrap();
            pool_connection
                .connection
                .connection_loop_tx
                .send(ConnectionLoopCommand::SendMessage(
                    irc!["JOIN", format!("#{}", channel_key.as_str())],
                    None,
                ))
                .unwrap();
            pool_connection.register_sent_message();
            return;
        }

        let cid = self
            .pop_non_full_connection_id()
            .unwrap_or_else(|| self.insert_new_connection(false));

        let arc_key = channel_key.into_arc();
        if let Some(state) = self.channel_index.get_mut(arc_key.as_ref()) {
            state.wanted_on = Some(cid);
        } else {
            self.channel_index.insert(
                Arc::clone(&arc_key),
                ChannelState {
                    wanted_on: Some(cid),
                    server_on: SmallVec::new(),
                },
            );
        }

        let pool_connection = self.connections.get_mut(&cid).unwrap();

        // delegate join command to connection
        pool_connection
            .connection
            .connection_loop_tx
            .send(ConnectionLoopCommand::SendMessage(
                irc!["JOIN", format!("#{}", arc_key)],
                None,
            ))
            .unwrap();

        pool_connection.register_sent_message();
        pool_connection.wanted_channels.insert(arc_key);
        #[cfg(feature = "metrics-collection")]
        {
            self.num_wanted_channels += 1;
        }

        if pool_connection.channels_limit_not_reached() {
            self.non_full_connections.push_back(cid);
        }

        if update_metrics {
            self.update_metrics();
        }
    }

    fn set_wanted_channels(&mut self, channels: HashSet<String>) {
        // part channels as needed
        let channels_to_part: Vec<Arc<str>> = self
            .channel_index
            .iter()
            .filter_map(|(channel_login, state)| {
                if state.wanted_on.is_some() && !channels.contains(channel_login.as_ref()) {
                    Some(Arc::clone(channel_login))
                } else {
                    None
                }
            })
            .collect();

        for channel_login in channels_to_part {
            self.part_inner(channel_login.as_ref(), false);
        }

        // join all wanted channels. Channels already joined will be detected
        // inside the join method.
        for channel_login in channels {
            self.join_inner(ChannelKey::Owned(channel_login), false);
        }

        self.update_metrics();
    }

    fn get_channel_status(&mut self, channel_login: String) -> (bool, bool) {
        self.channel_index
            .get(channel_login.as_str())
            .map_or((false, false), |state| {
                (state.wanted_on.is_some(), !state.server_on.is_empty())
            })
    }

    fn part(&mut self, channel_login: String) {
        self.part_inner(&channel_login, true);
    }

    /// Parts `channel_login` via `channel_index` in O(1), re-queuing the owning connection in
    /// `non_full_connections` if it was previously at `max_channels_per_connection`.
    /// Batch callers (`set_wanted_channels`) pass `update_metrics = false` and flush metrics once.
    fn part_inner(&mut self, channel_login: &str, update_metrics: bool) {
        // skip the PART altogether if the last message we sent regarding that channel was a PART
        // (or nothing at all, for that matter).
        let Some(state) = self.channel_index.get_mut(channel_login) else {
            return;
        };
        let Some(cid) = state.wanted_on.take() else {
            return;
        };
        if state.server_on.is_empty() {
            self.channel_index.remove(channel_login);
        }

        // now grab the connection that has that channel
        let pool_connection = self.connections.get_mut(&cid).unwrap();

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
        let was_full = !pool_connection.channels_limit_not_reached();
        let removed = pool_connection.wanted_channels.remove(channel_login);
        debug_assert!(removed);
        #[cfg(feature = "metrics-collection")]
        {
            self.num_wanted_channels -= 1;
        }

        if was_full && pool_connection.channels_limit_not_reached() {
            self.non_full_connections.push_back(cid);
        }

        if update_metrics {
            self.update_metrics();
        }
    }

    fn ping(&mut self, return_sender: oneshot::Sender<Result<(), Error<T, L>>>) {
        self.send_message(irc!["PING", "tmi.twitch.tv"], return_sender);
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
                    if let Some(current_whisper_connection_id) = self.current_whisper_connection_id
                    {
                        // another connection is already the chosen connection for whispers
                        // so we ignore this message if it doesn't come from that connection
                        if current_whisper_connection_id != source_connection_id {
                            tracing::debug!(
                                "Ignoring whisper from connection {} (not whisper connection)",
                                source_connection_id
                            );
                            return; // ignore message, don't forward.
                        }
                        tracing::debug!(
                            "Received whisper from connection {}, will be forwarded as it is the current whisper connection",
                            source_connection_id
                        );
                    } else {
                        // no connection chosen to be whisper connection yet
                        // since we just got a whisper, we will assign this connection to
                        // now be the responsible whisper connection. (and the message
                        // will be forwarded)
                        tracing::debug!(
                            "Received whisper and had no whisper connection selected. Selecting pool connection {}. Message was forwarded",
                            source_connection_id
                        );
                        self.current_whisper_connection_id = Some(source_connection_id);
                    }
                }

                match &*message {
                    ServerMessage::Join(JoinMessage { channel_login, .. }) => {
                        // we successfully joined a channel
                        let c = self.connections.get_mut(&source_connection_id).unwrap();
                        if !c.server_channels.contains(channel_login.as_str()) {
                            let key = c
                                .wanted_channels
                                .get(channel_login.as_str())
                                .cloned()
                                .or_else(|| {
                                    self.channel_index
                                        .get_key_value(channel_login.as_str())
                                        .map(|(k, _)| Arc::clone(k))
                                })
                                .unwrap_or_else(|| Arc::from(channel_login.as_str()));

                            c.server_channels.insert(Arc::clone(&key));
                            self.channel_index
                                .entry(key)
                                .or_default()
                                .server_on
                                .push(source_connection_id);
                            #[cfg(feature = "metrics-collection")]
                            {
                                self.num_server_channels += 1;
                            }

                            // update metrics about channel numbers
                            self.update_metrics();
                        }
                    }
                    ServerMessage::Part(PartMessage { channel_login, .. }) => {
                        // we successfully parted a channel
                        let c = self.connections.get_mut(&source_connection_id).unwrap();
                        if c.server_channels.remove(channel_login.as_str()) {
                            if let Some(state) = self.channel_index.get_mut(channel_login.as_str())
                            {
                                if let Some(pos) = state
                                    .server_on
                                    .iter()
                                    .position(|&id| id == source_connection_id)
                                {
                                    state.server_on.swap_remove(pos);
                                }
                                if state.wanted_on.is_none() && state.server_on.is_empty() {
                                    self.channel_index.remove(channel_login.as_str());
                                }
                            }
                            #[cfg(feature = "metrics-collection")]
                            {
                                self.num_server_channels -= 1;
                            }

                            // update metrics about channel numbers
                            self.update_metrics();
                        }
                    }
                    _ => {}
                }

                self.client_incoming_messages_tx.send(*message).ok(); // ignore if the library user is not using the incoming messages
            }
            #[cfg(feature = "metrics-collection")]
            ConnectionIncomingMessage::StateOpen => {
                let c = self.connections.get_mut(&source_connection_id).unwrap();
                if matches!(c.reported_state, ReportedConnectionState::Initializing) {
                    c.reported_state = ReportedConnectionState::Open;
                    self.num_initializing_connections -= 1;
                    self.num_open_connections += 1;
                    self.update_metrics();
                }
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
                let mut pool_connection = self.connections.remove(&source_connection_id).unwrap();
                if let Some(pos) = self
                    .send_order
                    .iter()
                    .position(|&id| id == source_connection_id)
                {
                    self.send_order.remove(pos);
                }
                if pool_connection.channels_limit_not_reached()
                    && let Some(pos) = self
                        .non_full_connections
                        .iter()
                        .position(|&id| id == source_connection_id)
                {
                    self.non_full_connections.remove(pos);
                }

                // count up failed connections counter and adjust channel/connection gauges
                #[cfg(feature = "metrics-collection")]
                {
                    match pool_connection.reported_state {
                        ReportedConnectionState::Initializing => {
                            self.num_initializing_connections -= 1;
                        }
                        ReportedConnectionState::Open => {
                            self.num_open_connections -= 1;
                        }
                    }
                    self.num_wanted_channels -= pool_connection.wanted_channels.len();
                    self.num_server_channels -= pool_connection.server_channels.len();
                    if let Some(ref metrics) = self.metrics {
                        metrics.connections_failed.inc();
                    }
                }

                for channel in &pool_connection.wanted_channels {
                    if let Some(state) = self.channel_index.get_mut(channel.as_ref()) {
                        if state.wanted_on == Some(source_connection_id) {
                            state.wanted_on = None;
                        }
                        if state.wanted_on.is_none() && state.server_on.is_empty() {
                            self.channel_index.remove(channel.as_ref());
                        }
                    }
                }
                for channel in &pool_connection.server_channels {
                    if let Some(state) = self.channel_index.get_mut(channel.as_ref()) {
                        if let Some(pos) = state
                            .server_on
                            .iter()
                            .position(|&id| id == source_connection_id)
                        {
                            state.server_on.swap_remove(pos);
                        }
                        if state.wanted_on.is_none() && state.server_on.is_empty() {
                            self.channel_index.remove(channel.as_ref());
                        }
                    }
                }

                // rejoin channels
                tracing::debug!(
                    "Pool connection {} previously was joined to {} channels ({:?}), rejoining them",
                    source_connection_id,
                    pool_connection.wanted_channels.len(),
                    pool_connection.wanted_channels
                );
                for channel in pool_connection.wanted_channels.drain() {
                    self.join_inner(ChannelKey::Shared(channel), false);
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
                    self.insert_new_connection(true);
                }

                // update twitch_irc_channels and twitch_irc_connections gauges
                self.update_metrics();
            }
        }
    }

    #[cfg(feature = "metrics-collection")]
    fn update_metrics(&mut self) {
        if let Some(ref metrics) = self.metrics {
            metrics
                .connections_initializing
                .set(self.num_initializing_connections as i64);
            metrics
                .connections_open
                .set(self.num_open_connections as i64);
            metrics.channels_wanted.set(self.num_wanted_channels as i64);
            metrics.channels_server.set(self.num_server_channels as i64);
        }
    }

    #[cfg(not(feature = "metrics-collection"))]
    fn update_metrics(&mut self) {}
}

#[cfg(test)]
mod tests {
    use crate::login::StaticLoginCredentials;
    use crate::message::{IRCMessage, IRCParseError, IRCPrefix, IRCTags, ServerMessage};
    use crate::transport::Transport;
    use crate::{ClientConfig, TwitchIRCClient};
    use async_trait::async_trait;
    use either::Either;
    use futures_util::sink::Sink;
    use futures_util::stream::{FusedStream, Stream};
    use std::collections::HashSet;
    use std::convert::Infallible;
    use std::pin::Pin;
    use std::sync::{Arc, LazyLock, Mutex};
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::sync::{Semaphore, mpsc};

    type IncomingItem = Result<IRCMessage, Either<Infallible, IRCParseError>>;

    static TEST_LOCK: LazyLock<tokio::sync::Mutex<()>> =
        LazyLock::new(|| tokio::sync::Mutex::new(()));
    static NEXT_TEST_CONN_TX: Mutex<Option<mpsc::UnboundedSender<TestConnHandle>>> =
        Mutex::new(None);

    #[derive(Debug)]
    struct TestConnHandle {
        incoming_tx: mpsc::UnboundedSender<IncomingItem>,
        outgoing_rx: mpsc::UnboundedReceiver<IRCMessage>,
    }

    impl TestConnHandle {
        fn confirm_join(&self, channel: &str) {
            let msg = IRCMessage::new(
                IRCTags::new(),
                Some(IRCPrefix::Full {
                    nick: "justinfan1".to_owned(),
                    user: Some("justinfan1".to_owned()),
                    host: Some("justinfan1.tmi.twitch.tv".to_owned()),
                }),
                "JOIN".to_owned(),
                vec![format!("#{channel}")],
            );
            self.incoming_tx.send(Ok(msg)).unwrap();
        }

        fn confirm_part(&self, channel: &str) {
            let msg = IRCMessage::new(
                IRCTags::new(),
                Some(IRCPrefix::Full {
                    nick: "justinfan1".to_owned(),
                    user: Some("justinfan1".to_owned()),
                    host: Some("justinfan1.tmi.twitch.tv".to_owned()),
                }),
                "PART".to_owned(),
                vec![format!("#{channel}")],
            );
            self.incoming_tx.send(Ok(msg)).unwrap();
        }

        async fn next_channel_cmd(&mut self) -> (String, String) {
            loop {
                let msg = self
                    .outgoing_rx
                    .recv()
                    .await
                    .expect("outgoing channel closed unexpectedly");
                if msg.command == "JOIN" || msg.command == "PART" {
                    return (msg.command, msg.params[0].clone());
                }
            }
        }
    }

    #[derive(Debug)]
    struct MockIncoming {
        rx: Mutex<mpsc::UnboundedReceiver<IncomingItem>>,
        terminated: bool,
    }

    impl Stream for MockIncoming {
        type Item = IncomingItem;

        fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            let this = self.get_mut();
            if this.terminated {
                return Poll::Ready(None);
            }
            match this.rx.get_mut().unwrap().poll_recv(cx) {
                Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
                Poll::Ready(None) => {
                    this.terminated = true;
                    Poll::Ready(None)
                }
                Poll::Pending => Poll::Pending,
            }
        }
    }

    impl FusedStream for MockIncoming {
        fn is_terminated(&self) -> bool {
            self.terminated
        }
    }

    #[derive(Debug)]
    struct MockOutgoing {
        tx: mpsc::UnboundedSender<IRCMessage>,
    }

    impl Sink<IRCMessage> for MockOutgoing {
        type Error = Infallible;

        fn poll_ready(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn start_send(self: Pin<&mut Self>, item: IRCMessage) -> Result<(), Self::Error> {
            let _ = self.tx.send(item);
            Ok(())
        }

        fn poll_flush(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
        ) -> Poll<Result<(), Self::Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[derive(Debug)]
    struct MockTransport {
        incoming: MockIncoming,
        outgoing: MockOutgoing,
    }

    #[async_trait]
    impl Transport for MockTransport {
        type ConnectError = Infallible;
        type IncomingError = Infallible;
        type OutgoingError = Infallible;
        type Incoming = MockIncoming;
        type Outgoing = MockOutgoing;

        async fn new() -> Result<Self, Self::ConnectError> {
            let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
            let (outgoing_tx, outgoing_rx) = mpsc::unbounded_channel();
            if let Some(tx) = NEXT_TEST_CONN_TX.lock().unwrap().as_ref() {
                let _ = tx.send(TestConnHandle {
                    incoming_tx,
                    outgoing_rx,
                });
            }
            Ok(MockTransport {
                incoming: MockIncoming {
                    rx: Mutex::new(incoming_rx),
                    terminated: false,
                },
                outgoing: MockOutgoing { tx: outgoing_tx },
            })
        }

        fn split(self) -> (Self::Incoming, Self::Outgoing) {
            (self.incoming, self.outgoing)
        }
    }

    struct MockGuard;

    impl Drop for MockGuard {
        fn drop(&mut self) {
            *NEXT_TEST_CONN_TX.lock().unwrap() = None;
        }
    }

    fn setup_mock_client(
        max_channels_per_connection: usize,
    ) -> (
        MockGuard,
        mpsc::UnboundedReceiver<TestConnHandle>,
        mpsc::UnboundedReceiver<ServerMessage>,
        TwitchIRCClient<MockTransport, StaticLoginCredentials>,
    ) {
        setup_mock_client_with_limiter(max_channels_per_connection, Arc::new(Semaphore::new(100)))
    }

    fn setup_mock_client_with_limiter(
        max_channels_per_connection: usize,
        connection_rate_limiter: Arc<Semaphore>,
    ) -> (
        MockGuard,
        mpsc::UnboundedReceiver<TestConnHandle>,
        mpsc::UnboundedReceiver<ServerMessage>,
        TwitchIRCClient<MockTransport, StaticLoginCredentials>,
    ) {
        let (conn_tx, conn_rx) = mpsc::unbounded_channel();
        *NEXT_TEST_CONN_TX.lock().unwrap() = Some(conn_tx);
        let config = ClientConfig {
            max_channels_per_connection,
            connection_rate_limiter,
            new_connection_every: Duration::ZERO,
            #[cfg(feature = "metrics-collection")]
            metrics_config: crate::MetricsConfig::Disabled,
            ..ClientConfig::default()
        };
        let (incoming_messages, client) =
            TwitchIRCClient::<MockTransport, StaticLoginCredentials>::new(config);
        (MockGuard, conn_rx, incoming_messages, client)
    }

    async fn wait_for_join_event(
        incoming_messages: &mut mpsc::UnboundedReceiver<ServerMessage>,
        expected_channel: &str,
    ) {
        while let Some(msg) = incoming_messages.recv().await {
            if let ServerMessage::Join(j) = msg
                && j.channel_login == expected_channel
            {
                return;
            }
        }
        panic!("incoming stream closed before JOIN {expected_channel}");
    }

    async fn wait_for_part_event(
        incoming_messages: &mut mpsc::UnboundedReceiver<ServerMessage>,
        expected_channel: &str,
    ) {
        while let Some(msg) = incoming_messages.recv().await {
            if let ServerMessage::Part(p) = msg
                && p.channel_login == expected_channel
            {
                return;
            }
        }
        panic!("incoming stream closed before PART {expected_channel}");
    }

    async fn shutdown_client(
        client: TwitchIRCClient<MockTransport, StaticLoginCredentials>,
        mut incoming_messages: mpsc::UnboundedReceiver<ServerMessage>,
    ) {
        drop(client);
        while incoming_messages.recv().await.is_some() {}
    }

    #[tokio::test]
    async fn join_confirm_part_lifecycle_and_skip_confirmed() {
        let _lock = TEST_LOCK.lock().await;
        let (_guard, mut conn_rx, mut incoming, client) = setup_mock_client(90);

        assert_eq!(
            client.get_channel_status("alpha".to_owned()).await,
            (false, false)
        );

        client.join("alpha".to_owned()).unwrap();
        let mut conn = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn.next_channel_cmd().await,
            ("JOIN".to_owned(), "#alpha".to_owned())
        );
        assert_eq!(
            client.get_channel_status("alpha".to_owned()).await,
            (true, false)
        );

        conn.confirm_join("alpha");
        wait_for_join_event(&mut incoming, "alpha").await;
        assert_eq!(
            client.get_channel_status("alpha".to_owned()).await,
            (true, true)
        );

        // Second join on already-confirmed channel should not send another JOIN.
        client.join("alpha".to_owned()).unwrap();
        client.part("alpha".to_owned());
        assert_eq!(
            conn.next_channel_cmd().await,
            ("PART".to_owned(), "#alpha".to_owned())
        );
        assert_eq!(
            client.get_channel_status("alpha".to_owned()).await,
            (false, true)
        );

        conn.confirm_part("alpha");
        wait_for_part_event(&mut incoming, "alpha").await;
        assert_eq!(
            client.get_channel_status("alpha".to_owned()).await,
            (false, false)
        );

        shutdown_client(client, incoming).await;
    }

    #[tokio::test]
    async fn set_wanted_channels_skips_confirmed_retries_unconfirmed_and_parts_removed() {
        let _lock = TEST_LOCK.lock().await;
        let (_guard, mut conn_rx, mut incoming, client) = setup_mock_client(90);

        client
            .set_wanted_channels(HashSet::from([
                "confirmed_ch".to_owned(),
                "unconfirmed_ch".to_owned(),
                "removed_ch".to_owned(),
            ]))
            .unwrap();

        let mut conn = conn_rx.recv().await.unwrap();
        let mut initial_cmds = HashSet::new();
        for _ in 0..3 {
            initial_cmds.insert(conn.next_channel_cmd().await);
        }
        assert_eq!(
            initial_cmds,
            HashSet::from([
                ("JOIN".to_owned(), "#confirmed_ch".to_owned()),
                ("JOIN".to_owned(), "#unconfirmed_ch".to_owned()),
                ("JOIN".to_owned(), "#removed_ch".to_owned()),
            ])
        );

        conn.confirm_join("confirmed_ch");
        conn.confirm_join("removed_ch");
        wait_for_join_event(&mut incoming, "confirmed_ch").await;
        wait_for_join_event(&mut incoming, "removed_ch").await;

        client
            .set_wanted_channels(HashSet::from([
                "confirmed_ch".to_owned(),
                "unconfirmed_ch".to_owned(),
                "added_ch".to_owned(),
            ]))
            .unwrap();

        let mut next_cmds = HashSet::new();
        for _ in 0..3 {
            next_cmds.insert(conn.next_channel_cmd().await);
        }
        assert_eq!(
            next_cmds,
            HashSet::from([
                ("PART".to_owned(), "#removed_ch".to_owned()),
                ("JOIN".to_owned(), "#unconfirmed_ch".to_owned()),
                ("JOIN".to_owned(), "#added_ch".to_owned()),
            ])
        );
        // Sentinel command on the FIFO outgoing stream guarantees no extra JOINs were emitted.
        client.part("added_ch".to_owned());
        assert_eq!(
            conn.next_channel_cmd().await,
            ("PART".to_owned(), "#added_ch".to_owned())
        );

        shutdown_client(client, incoming).await;
    }

    #[tokio::test]
    async fn full_connection_becomes_reusable_after_part() {
        let _lock = TEST_LOCK.lock().await;
        let (_guard, mut conn_rx, incoming, client) = setup_mock_client(2);

        client.join("c1".to_owned()).unwrap();
        let mut conn0 = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("JOIN".to_owned(), "#c1".to_owned())
        );

        client.join("c2".to_owned()).unwrap();
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("JOIN".to_owned(), "#c2".to_owned())
        );

        // Third channel forces a second connection because conn0 reached max_channels_per_connection=2.
        client.join("c3".to_owned()).unwrap();
        let mut conn1 = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn1.next_channel_cmd().await,
            ("JOIN".to_owned(), "#c3".to_owned())
        );

        // Parting from full conn0 makes conn0 non-full again.
        client.part("c1".to_owned());
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("PART".to_owned(), "#c1".to_owned())
        );

        // Joining c4 fills conn1 (1 -> 2) and c5 fills conn0 (1 -> 2), without opening conn2.
        client.join("c4".to_owned()).unwrap();
        client.join("c5".to_owned()).unwrap();
        assert_eq!(
            conn1.next_channel_cmd().await,
            ("JOIN".to_owned(), "#c4".to_owned())
        );
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("JOIN".to_owned(), "#c5".to_owned())
        );
        assert!(conn_rx.try_recv().is_err());

        shutdown_client(client, incoming).await;
    }

    #[tokio::test]
    async fn transient_part_and_rejoin_across_connections_preserves_server_status() {
        let _lock = TEST_LOCK.lock().await;
        let (_guard, mut conn_rx, mut incoming, client) = setup_mock_client(1);

        client.join("chan".to_owned()).unwrap();
        let mut conn0 = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("JOIN".to_owned(), "#chan".to_owned())
        );
        conn0.confirm_join("chan");
        wait_for_join_event(&mut incoming, "chan").await;
        assert_eq!(
            client.get_channel_status("chan".to_owned()).await,
            (true, true)
        );

        // Occupy a second connection so when `chan` is parted without server PART yet,
        // wait: conn0 becomes non-full on part, so fill conn0 first with `other` after parting `chan`.
        client.part("chan".to_owned());
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("PART".to_owned(), "#chan".to_owned())
        );
        // Server hasn't confirmed PART yet:
        assert_eq!(
            client.get_channel_status("chan".to_owned()).await,
            (false, true)
        );

        client.join("other".to_owned()).unwrap();
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("JOIN".to_owned(), "#other".to_owned())
        );

        // Now re-join `chan`: conn0 is full (`other`), so `chan` goes onto conn1.
        client.join("chan".to_owned()).unwrap();
        let mut conn1 = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn1.next_channel_cmd().await,
            ("JOIN".to_owned(), "#chan".to_owned())
        );
        conn1.confirm_join("chan");
        wait_for_join_event(&mut incoming, "chan").await;
        assert_eq!(
            client.get_channel_status("chan".to_owned()).await,
            (true, true)
        );

        // Now delayed PART arrives from conn0: status must remain (true, true) via conn1!
        conn0.confirm_part("chan");
        wait_for_part_event(&mut incoming, "chan").await;
        assert_eq!(
            client.get_channel_status("chan".to_owned()).await,
            (true, true)
        );

        shutdown_client(client, incoming).await;
    }

    #[tokio::test]
    async fn state_closed_rejoins_wanted_channels_and_resets_server_confirmation() {
        let _lock = TEST_LOCK.lock().await;
        let (_guard, mut conn_rx, mut incoming, client) = setup_mock_client(90);

        client.join("persist".to_owned()).unwrap();
        let mut conn0 = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("JOIN".to_owned(), "#persist".to_owned())
        );
        conn0.confirm_join("persist");
        wait_for_join_event(&mut incoming, "persist").await;
        assert_eq!(
            client.get_channel_status("persist".to_owned()).await,
            (true, true)
        );

        // Close conn0 by dropping its incoming sender (EOF).
        drop(conn0);

        // ClientLoopWorker should spawn conn1 and re-send JOIN #persist.
        let mut conn1 = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn1.next_channel_cmd().await,
            ("JOIN".to_owned(), "#persist".to_owned())
        );
        assert_eq!(
            client.get_channel_status("persist".to_owned()).await,
            (true, false)
        );

        conn1.confirm_join("persist");
        wait_for_join_event(&mut incoming, "persist").await;
        assert_eq!(
            client.get_channel_status("persist".to_owned()).await,
            (true, true)
        );

        shutdown_client(client, incoming).await;
    }

    #[tokio::test]
    async fn initializing_connection_does_not_queue_duplicate_joins_on_retry_or_set_wanted_channels()
     {
        let _lock = TEST_LOCK.lock().await;
        let limiter = Arc::new(Semaphore::new(0));
        let (_guard, mut conn_rx, incoming, client) =
            setup_mock_client_with_limiter(90, Arc::clone(&limiter));

        client.join("a".to_owned()).unwrap();
        client.join("a".to_owned()).unwrap();
        client
            .set_wanted_channels(HashSet::from(["a".to_owned(), "b".to_owned()]))
            .unwrap();
        client
            .set_wanted_channels(HashSet::from(["a".to_owned(), "b".to_owned()]))
            .unwrap();
        client.join("b".to_owned()).unwrap();

        assert_eq!(
            client.get_channel_status("a".to_owned()).await,
            (true, false)
        );
        assert_eq!(
            client.get_channel_status("b".to_owned()).await,
            (true, false)
        );
        assert!(conn_rx.try_recv().is_err());

        limiter.add_permits(1);
        let mut conn0 = conn_rx.recv().await.unwrap();
        let mut cmds = HashSet::new();
        for _ in 0..2 {
            cmds.insert(conn0.next_channel_cmd().await);
        }
        assert_eq!(
            cmds,
            HashSet::from([
                ("JOIN".to_owned(), "#a".to_owned()),
                ("JOIN".to_owned(), "#b".to_owned()),
            ])
        );

        // Sentinel command on the FIFO outgoing stream guarantees no duplicate JOINs were sent.
        client.part("b".to_owned());
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("PART".to_owned(), "#b".to_owned())
        );

        shutdown_client(client, incoming).await;
    }

    #[tokio::test]
    async fn part_while_initializing_suppresses_both_join_and_part_on_open() {
        let _lock = TEST_LOCK.lock().await;
        let limiter = Arc::new(Semaphore::new(0));
        let (_guard, mut conn_rx, incoming, client) =
            setup_mock_client_with_limiter(90, Arc::clone(&limiter));

        client.join("stay".to_owned()).unwrap();
        client.join("leave".to_owned()).unwrap();
        client.part("leave".to_owned());

        assert_eq!(
            client.get_channel_status("leave".to_owned()).await,
            (false, false)
        );
        assert_eq!(
            client.get_channel_status("stay".to_owned()).await,
            (true, false)
        );

        limiter.add_permits(1);
        let mut conn0 = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("JOIN".to_owned(), "#stay".to_owned())
        );

        // Sentinel: `JOIN #stay` guarantees `StateOpen` was already processed, so `part("stay")`
        // runs on the open connection and must immediately follow `JOIN #stay` with no `leave` cmds.
        client.part("stay".to_owned());
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("PART".to_owned(), "#stay".to_owned())
        );

        shutdown_client(client, incoming).await;
    }

    #[tokio::test]
    async fn part_while_initializing_frees_slot_for_new_channel_without_extra_connection() {
        let _lock = TEST_LOCK.lock().await;
        let limiter = Arc::new(Semaphore::new(0));
        let (_guard, mut conn_rx, incoming, client) =
            setup_mock_client_with_limiter(2, Arc::clone(&limiter));

        client.join("c1".to_owned()).unwrap();
        client.join("c2".to_owned()).unwrap();
        client.part("c1".to_owned());
        client.join("c3".to_owned()).unwrap();

        assert_eq!(
            client.get_channel_status("c1".to_owned()).await,
            (false, false)
        );
        assert_eq!(
            client.get_channel_status("c3".to_owned()).await,
            (true, false)
        );

        limiter.add_permits(2);
        let mut conn0 = conn_rx.recv().await.unwrap();
        let mut cmds = HashSet::new();
        for _ in 0..2 {
            cmds.insert(conn0.next_channel_cmd().await);
        }
        assert_eq!(
            cmds,
            HashSet::from([
                ("JOIN".to_owned(), "#c2".to_owned()),
                ("JOIN".to_owned(), "#c3".to_owned()),
            ])
        );

        assert_eq!(
            client.get_channel_status("c3".to_owned()).await,
            (true, false)
        );
        assert!(conn_rx.try_recv().is_err());

        shutdown_client(client, incoming).await;
    }

    #[tokio::test]
    async fn join_part_rejoin_while_initializing_sends_single_join_on_open() {
        let _lock = TEST_LOCK.lock().await;
        let limiter = Arc::new(Semaphore::new(0));
        let (_guard, mut conn_rx, incoming, client) =
            setup_mock_client_with_limiter(90, Arc::clone(&limiter));

        client.join("ch".to_owned()).unwrap();
        client.part("ch".to_owned());
        client.join("ch".to_owned()).unwrap();

        assert_eq!(
            client.get_channel_status("ch".to_owned()).await,
            (true, false)
        );

        limiter.add_permits(1);
        let mut conn0 = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("JOIN".to_owned(), "#ch".to_owned())
        );

        client.part("ch".to_owned());
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("PART".to_owned(), "#ch".to_owned())
        );

        shutdown_client(client, incoming).await;
    }

    #[cfg(feature = "metrics-collection")]
    #[tokio::test]
    async fn incremental_channel_and_connection_metrics_stay_accurate() {
        use crate::MetricsConfig;
        use std::collections::HashMap;

        let _lock = TEST_LOCK.lock().await;
        let (conn_tx, mut conn_rx) = mpsc::unbounded_channel();
        *NEXT_TEST_CONN_TX.lock().unwrap() = Some(conn_tx);
        let _guard = MockGuard;

        let registry = prometheus::Registry::new();
        let config = ClientConfig {
            connection_rate_limiter: Arc::new(Semaphore::new(100)),
            new_connection_every: Duration::ZERO,
            metrics_config: MetricsConfig::Enabled {
                constant_labels: HashMap::new(),
                metrics_registry: Some(registry.clone()),
            },
            ..ClientConfig::default()
        };
        let (mut incoming, client) =
            TwitchIRCClient::<MockTransport, StaticLoginCredentials>::new(config);

        let read_gauge = |family_name: &str, label_name: &str, label_val: &str| -> i64 {
            let families = registry.gather();
            let fam = families.iter().find(|f| f.name() == family_name).unwrap();
            fam.get_metric()
                .iter()
                .find(|m| {
                    m.get_label()
                        .iter()
                        .any(|l| l.name() == label_name && l.value() == label_val)
                })
                .unwrap()
                .get_gauge()
                .get_value() as i64
        };

        client
            .set_wanted_channels(HashSet::from(["m1".to_owned(), "m2".to_owned()]))
            .unwrap();
        let mut conn0 = conn_rx.recv().await.unwrap();
        let _ = conn0.next_channel_cmd().await;
        let _ = conn0.next_channel_cmd().await;
        let _ = client.get_channel_status("m1".to_owned()).await;

        assert_eq!(read_gauge("twitchirc_channels", "type", "wanted"), 2);
        assert_eq!(read_gauge("twitchirc_channels", "type", "server"), 0);

        // `StateOpen` is sent on the connection's FIFO incoming stream before any `JOIN`
        // confirmation can be processed, so waiting for `m1` guarantees `StateOpen` arrived.
        conn0.confirm_join("m1");
        wait_for_join_event(&mut incoming, "m1").await;
        assert_eq!(read_gauge("twitchirc_connections", "state", "open"), 1);
        assert_eq!(
            read_gauge("twitchirc_connections", "state", "initializing"),
            0
        );
        assert_eq!(read_gauge("twitchirc_channels", "type", "wanted"), 2);
        assert_eq!(read_gauge("twitchirc_channels", "type", "server"), 1);

        // Part m2 (unconfirmed) and m1 (confirmed).
        client
            .set_wanted_channels(HashSet::from(["m1".to_owned()]))
            .unwrap();
        assert_eq!(
            conn0.next_channel_cmd().await,
            ("PART".to_owned(), "#m2".to_owned())
        );
        let _ = client.get_channel_status("m1".to_owned()).await;
        assert_eq!(read_gauge("twitchirc_channels", "type", "wanted"), 1);
        assert_eq!(read_gauge("twitchirc_channels", "type", "server"), 1);

        // Trigger StateClosed on conn0: m1 stays wanted (rejoined on conn1), server count resets to 0.
        drop(conn0);
        let mut conn1 = conn_rx.recv().await.unwrap();
        assert_eq!(
            conn1.next_channel_cmd().await,
            ("JOIN".to_owned(), "#m1".to_owned())
        );
        let _ = client.get_channel_status("m1".to_owned()).await;
        assert_eq!(read_gauge("twitchirc_channels", "type", "wanted"), 1);
        assert_eq!(read_gauge("twitchirc_channels", "type", "server"), 0);

        conn1.confirm_join("m1");
        wait_for_join_event(&mut incoming, "m1").await;
        assert_eq!(read_gauge("twitchirc_channels", "type", "wanted"), 1);
        assert_eq!(read_gauge("twitchirc_channels", "type", "server"), 1);

        shutdown_client(client, incoming).await;
    }
}
