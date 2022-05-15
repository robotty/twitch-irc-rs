pub mod event_loop;

use crate::config::ClientConfig;
use crate::connection::event_loop::{ConnectionLoopCommand, ConnectionLoopWorker};
use crate::error::Error;
use crate::login::LoginCredentials;
use crate::message::commands::ServerMessage;
#[cfg(feature = "metrics-collection")]
use crate::metrics::MetricsBundle;
use crate::transport::Transport;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum ConnectionIncomingMessage<T: Transport, L: LoginCredentials> {
    IncomingMessage(ServerMessage),
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
    /// makes a tuple with the incoming messages and the `Connection` handle for outgoing
    /// messages.
    pub fn new(
        config: Arc<ClientConfig<L>>,
        connection_id: usize,
        #[cfg(feature = "metrics-collection")] metrics: Option<MetricsBundle>,
    ) -> (
        mpsc::UnboundedReceiver<ConnectionIncomingMessage<T, L>>,
        Connection<T, L>,
    ) {
        let (connection_loop_tx, connection_loop_rx) = mpsc::unbounded_channel();
        let (connection_incoming_tx, connection_incoming_rx) = mpsc::unbounded_channel();
        let connection_loop_tx = Arc::new(connection_loop_tx);

        ConnectionLoopWorker::spawn(
            config,
            connection_incoming_tx,
            Arc::downgrade(&connection_loop_tx),
            connection_loop_rx,
            connection_id,
            #[cfg(feature = "metrics-collection")]
            metrics,
        );

        (connection_incoming_rx, Connection { connection_loop_tx })
    }
}
