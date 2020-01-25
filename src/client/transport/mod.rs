mod message_stream;
mod tcp;
mod transport_error;
mod ws;

use crate::message::IRCMessage;
use futures::prelude::*;
pub use message_stream::MessageStream;
pub use tcp::TCPTransport;
pub use transport_error::TransportError;
pub use ws::WSTransport;

trait Transport {
    fn split(
        self,
    ) -> (
        Box<dyn Stream<Item = Result<IRCMessage, TransportError>>>,
        Box<dyn Sink<IRCMessage, Error = std::io::Error>>,
    );
}
