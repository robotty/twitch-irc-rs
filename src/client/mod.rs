mod config;
mod connection;
mod operations;
mod pool;
mod transport;

use self::pool::ConnectionPool;
use self::transport::Transport;
use crate::client::config::{ClientConfig, LoginCredentials};
use crate::message::IRCMessage;
use futures::channel::mpsc::Receiver;
use std::sync::Arc;

struct TwitchIRCClient<T: Transport, L: LoginCredentials> {
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

    fn incoming_messages() -> Receiver<Result<IRCMessage, T::IncomingError>> {
        todo!()
    }
}
