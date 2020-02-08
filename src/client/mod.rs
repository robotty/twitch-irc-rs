pub mod config;
pub mod connection;
pub mod operations;
pub mod pool;
pub mod transport;

use self::pool::ConnectionPool;
use self::transport::Transport;
use crate::client::config::{ClientConfig, LoginCredentials};
use crate::client::connection::Connection;
use crate::client::pool::ConnectionInitError;
use crate::message::IRCMessage;
use futures::channel::mpsc::Receiver;
use std::sync::Arc;

pub struct TwitchIRCClient<T: Transport, L: LoginCredentials> {
    connection_pool: ConnectionPool<T, L>,
    config: Arc<ClientConfig<L>>,
}

impl<T: Transport, L: LoginCredentials> TwitchIRCClient<T, L> {
    pub fn new(config: ClientConfig<L>) -> TwitchIRCClient<T, L> {
        let config = Arc::new(config);

        TwitchIRCClient {
            connection_pool: ConnectionPool::new(Arc::clone(&config)),
            config,
        }
    }

    pub fn take_incoming_messages(
        &mut self,
    ) -> Option<Receiver<Result<IRCMessage, T::IncomingError>>> {
        self.connection_pool.incoming_messages.take()
    }

    pub async fn checkout_connection(
        &self,
    ) -> Result<
        Arc<Connection<T, L>>,
        Arc<ConnectionInitError<T::ConnectError, L::Error, T::OutgoingError>>,
    > {
        self.connection_pool.checkout_connection().await
    }
}
