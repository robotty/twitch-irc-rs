use crate::config::ClientConfig;
use crate::connection::Connection;
use crate::login::LoginCredentials;
use crate::transport::Transport;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::oneshot;

/// The actual state of the connection loop is held only by the connection loop.
/// However the connection sends out messages indicating that it has changed its state.
/// This enum tracks that "reported state" as received via messages from the connection.
///
/// (The only use of this is to be able to provide metrics counting channels on a per-state basis)
pub(crate) enum ReportedConnectionState {
    Initializing,
    Open,
}

pub(crate) struct PoolConnection<T: Transport, L: LoginCredentials> {
    config: Arc<ClientConfig<L>>,
    /// uniquely identifies this pool connection within its parent pool. This ID is assigned
    /// by the pool.
    ///
    /// this is a `usize` because we can't possibly have more than `usize` connections at one point
    /// anyways, since our collections can't store more than that many (also it's an unrealistically
    /// high number anyways)
    pub id: usize,
    /// The connection handle that this is wrapping
    pub connection: Arc<Connection<T, L>>,
    /// see the documentation on `TwitchIRCClient` for what `wanted_channels` and `server_channels` mean
    pub wanted_channels: HashSet<String>,
    /// see the documentation on `TwitchIRCClient` for what `wanted_channels` and `server_channels` mean
    pub server_channels: HashSet<String>,
    /// this has a list of times when messages were sent out on this pool connection,
    /// at the front there will be the oldest, and at the back the newest entries
    pub message_send_times: VecDeque<Instant>,
    /// The actual state of the connection loop is held only by the connection loop.
    /// However the connection sends out messages indicating that it has changed its state.
    /// This enum tracks that "reported state" as received via messages from the connection.
    ///
    /// (The only use of this is to be able to provide metrics counting channels on a per-state basis)
    pub reported_state: ReportedConnectionState,

    // this is option-wrapped so it can be .take()n in the Drop implementation
    tx_kill_incoming: Option<oneshot::Sender<()>>,
}

impl<T: Transport, L: LoginCredentials> PoolConnection<T, L> {
    pub fn new(
        config: Arc<ClientConfig<L>>,
        id: usize,
        connection: Connection<T, L>,
        tx_kill_incoming: oneshot::Sender<()>,
    ) -> PoolConnection<T, L> {
        // this is just an optimization to initialize the VecDeque to its final size right away
        let message_send_times_max_entries = config.max_waiting_messages_per_connection * 2;
        PoolConnection {
            config,
            id,
            connection: Arc::new(connection),
            wanted_channels: HashSet::new(),
            server_channels: HashSet::new(),
            message_send_times: VecDeque::with_capacity(message_send_times_max_entries),
            reported_state: ReportedConnectionState::Initializing,
            tx_kill_incoming: Some(tx_kill_incoming),
        }
    }

    pub fn register_sent_message(&mut self) {
        let max_entries = self.config.max_waiting_messages_per_connection * 2;

        self.message_send_times.push_back(Instant::now());

        if self.message_send_times.len() > max_entries {
            self.message_send_times.pop_front();
        }
    }

    pub fn channels_limit_not_reached(&self) -> bool {
        let configured_limit = self.config.max_channels_per_connection;
        self.wanted_channels.len() < configured_limit
    }

    pub fn not_busy(&self) -> bool {
        let time_per_message = self.config.time_per_message;
        let max_waiting_per_connection = self.config.max_waiting_messages_per_connection;

        let mut messages_waiting = self.message_send_times.len();
        let current_time = Instant::now();
        let last_message_finished = None;
        // front: oldest send times, back: newest send times, and iter() goes front-to-back.
        for send_time in self.message_send_times.iter() {
            // the time when the server has begun or will begin processing this message.
            let start_time = match last_message_finished {
                Some(last_message_finished) => std::cmp::max(last_message_finished, send_time),
                None => send_time,
            };

            // the message will be fully processed by this time.
            let finish_time = *start_time + time_per_message;

            if finish_time < current_time {
                messages_waiting -= 1;
            } else {
                // this message (and consequently all after it) have not been processed yet by the
                // server, so they are the waiting messages.
                break;
            }
        }

        messages_waiting < max_waiting_per_connection
    }
}

impl<T: Transport, L: LoginCredentials> Drop for PoolConnection<T, L> {
    fn drop(&mut self) {
        // kill the incoming messages forwarder
        self.tx_kill_incoming.take().unwrap().send(()).ok();
    }
}
