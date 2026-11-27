pub mod event_loop;
mod outgoing;

use crate::client::event_loop::ClientLoopCommand;
use crate::config::ClientConfig;
use crate::connection::event_loop::{ConnectionLoopCommand, ConnectionLoopWorker};
use crate::error::Error;
use crate::login::LoginCredentials;
use crate::message::commands::ServerMessage;
#[cfg(feature = "metrics-collection")]
use crate::metrics::MetricsBundle;
use crate::transport::Transport;
use std::sync::{Arc, Weak};
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum ConnectionIncomingMessage<T: Transport, L: LoginCredentials> {
    IncomingMessage(Box<ServerMessage>),
    #[cfg(feature = "metrics-collection")]
    StateOpen,
    StateClosed {
        cause: Error<T, L>,
    },
}

pub(crate) struct Connection<T: Transport, L: LoginCredentials> {
    /// sends commands to the this connection's event loop.
    pub connection_loop_tx: Arc<mpsc::UnboundedSender<ConnectionLoopCommand<T, L>>>,
}

impl<T: Transport, L: LoginCredentials> Connection<T, L> {
    /// Spawns a connection worker that forwards incoming events and state changes directly to
    /// `client_loop_tx`, returning the `Connection` handle for outgoing messages.
    pub fn new(
        config: Arc<ClientConfig<L>>,
        connection_id: usize,
        client_loop_tx: Weak<mpsc::UnboundedSender<ClientLoopCommand<T, L>>>,
        #[cfg(feature = "metrics-collection")] metrics: Option<MetricsBundle>,
    ) -> Connection<T, L> {
        let (connection_loop_tx, connection_loop_rx) = mpsc::unbounded_channel();
        let connection_loop_tx = Arc::new(connection_loop_tx);

        ConnectionLoopWorker::spawn(
            config,
            client_loop_tx,
            connection_loop_rx,
            connection_id,
            #[cfg(feature = "metrics-collection")]
            metrics,
        );

        Connection { connection_loop_tx }
    }
}
