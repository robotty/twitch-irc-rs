mod pool;
mod transport;

use self::pool::ConnectionPool;
use self::transport::Transport;

struct TwitchIRCClient<T: Transport> {
    connection_pool: ConnectionPool<T>,
}

impl<T: Transport> TwitchIRCClient<T> {
    pub fn new() -> TwitchIRCClient<T> {
        TwitchIRCClient {
            connection_pool: ConnectionPool::new(),
        }
    }
}
