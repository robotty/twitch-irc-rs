mod message_stream;
mod transport_error;

use crate::message::AsRawIRC;
use crate::message::IRCMessage;
use bytes::Bytes;
use futures::future::ready;
use futures::prelude::*;
use futures::prelude::*;
pub use message_stream::MessageStream;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio_util::codec::{BytesCodec, FramedWrite};
pub use transport_error::TransportError;

pub struct Transport {
    pub incoming_messages: Box<dyn Stream<Item = Result<IRCMessage, TransportError>>>,
    pub outgoing_messages: Box<dyn Sink<IRCMessage, Error = std::io::Error>>,
}

async fn new_tcp() -> std::io::Result<Transport> {
    let socket = TcpStream::connect("irc.chat.twitch.tv:6667").await?;

    let (read_half, write_half) = tokio::io::split(socket);

    let message_stream = MessageStream::new(BufReader::new(read_half).lines());
    let message_sink = FramedWrite::new(write_half, BytesCodec::new())
        .with(|str: String| ready(Ok::<Bytes, std::io::Error>(Bytes::from(str))))
        .with(|msg: IRCMessage| ready(Ok::<String, std::io::Error>(msg.as_raw_irc())));

    Ok(Transport {
        incoming_messages: Box::new(message_stream),
        outgoing_messages: Box::new(message_sink),
    })
}
