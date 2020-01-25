use crate::message::AsRawIRC;
use crate::message::IRCMessage;
use crate::message::IRCParseError;
use async_tungstenite::tokio::connect_async;
use bytes::Bytes;
use futures::future::ready;
use futures::prelude::*;
use futures::stream::TryStreamExt;
use smallvec::SmallVec;
use std::convert::From;
use thiserror::Error;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio_util::codec::{BytesCodec, FramedWrite};
use tungstenite::Error as WSError;
use tungstenite::Message as WSMessage;
use url::Url;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    WebSocketError(#[from] WSError),
    #[error("{0}")]
    IRCParseError(#[from] IRCParseError),
}

pub struct Transport {
    pub incoming_messages: Box<dyn Stream<Item = Result<IRCMessage, TransportError>>>,
    pub outgoing_messages: Box<dyn Sink<IRCMessage, Error = TransportError>>,
}

async fn new_tcp() -> std::io::Result<Transport> {
    let socket = TcpStream::connect("irc.chat.twitch.tv:6667").await?;

    let (read_half, write_half) = tokio::io::split(socket);

    let message_stream = BufReader::new(read_half)
        .lines()
        .map_err(TransportError::from)
        .and_then(|s| ready(IRCMessage::parse(&s).map_err(TransportError::from)));
    let message_sink = FramedWrite::new(write_half, BytesCodec::new())
        .with(|s: String| ready(Ok::<Bytes, TransportError>(Bytes::from(s))))
        .with(|msg: IRCMessage| {
            let mut s = msg.as_raw_irc();
            s.push_str("\r\n");
            ready(Ok::<String, TransportError>(s))
        });

    Ok(Transport {
        incoming_messages: Box::new(message_stream),
        outgoing_messages: Box::new(message_sink),
    })
}

async fn new_ws() -> Result<Transport, tungstenite::error::Error> {
    let (ws_stream, _response) =
        connect_async(Url::parse("wss://irc-ws.chat.twitch.tv").unwrap()).await?;

    let (write_half, read_half) = futures::stream::StreamExt::split(ws_stream);

    let message_stream = read_half
        .map_err(TransportError::from)
        .try_filter_map(|ws_message| {
            ready(Ok::<_, TransportError>(
                if let WSMessage::Text(text) = ws_message {
                    Some(futures::stream::iter(
                        text.lines()
                            .map(|l| Ok(String::from(l)))
                            .collect::<SmallVec<[Result<String, _>; 1]>>(),
                    ))
                } else {
                    None
                },
            ))
        })
        .try_flatten()
        .and_then(|s| ready(IRCMessage::parse(&s).map_err(TransportError::from)));

    let message_sink = write_half
        .with(|str: String| ready(Ok::<WSMessage, TransportError>(WSMessage::Text(str))))
        .with(|msg: IRCMessage| ready(Ok::<String, TransportError>(msg.as_raw_irc())));

    Ok(Transport {
        incoming_messages: Box::new(message_stream),
        outgoing_messages: Box::new(message_sink),
    })
}
