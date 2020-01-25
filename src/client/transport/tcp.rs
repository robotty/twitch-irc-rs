use super::message_stream::MessageStream;
use crate::message::{AsRawIRC, IRCMessage};
use bytes::Bytes;
use futures::future::ready;
use futures::prelude::*;
use tokio::io::Lines;
use tokio::io::{BufReader, ReadHalf};
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio_util::codec::{BytesCodec, FramedWrite};

pub struct TCPTransport {
    pub incoming_messages: MessageStream<Lines<BufReader<ReadHalf<TcpStream>>>>,
    pub outgoing_messages: Box<dyn Sink<IRCMessage, Error = std::io::Error>>,
}

impl TCPTransport {
    pub async fn new() -> std::io::Result<TCPTransport> {
        let socket = TcpStream::connect("irc.chat.twitch.tv:6667").await?;

        let (read_half, write_half) = tokio::io::split(socket);

        let buf_reader = BufReader::new(read_half);
        let lines = buf_reader.lines();
        let message_stream = MessageStream::new(lines);

        let byte_sink = FramedWrite::new(write_half, BytesCodec::new());
        let str_sink =
            byte_sink.with(|str: String| ready(Ok::<Bytes, std::io::Error>(Bytes::from(str))));
        let message_sink =
            str_sink.with(|msg: IRCMessage| ready(Ok::<String, std::io::Error>(msg.as_raw_irc())));

        Ok(TCPTransport {
            incoming_messages: message_stream,
            outgoing_messages: Box::new(message_sink),
        })
    }
}
