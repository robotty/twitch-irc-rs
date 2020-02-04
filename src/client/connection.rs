use super::transport::Transport;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Connection<T: Transport> {
    pub incoming_messages: Arc<Mutex<T::Incoming>>,
    pub outgoing_messages: Arc<Mutex<T::Outgoing>>,
    pub channels: Vec<String>,
}

impl<T: Transport> From<T> for Connection<T> {
    fn from(transport: T) -> Self {
        // destructure the given transport...
        let (incoming_messages, outgoing_messages) = transport.split();

        // and build a Connection from the parts
        Connection {
            incoming_messages: Arc::new(Mutex::new(incoming_messages)),
            outgoing_messages: Arc::new(Mutex::new(outgoing_messages)),
            channels: vec![],
        }
    }
}
