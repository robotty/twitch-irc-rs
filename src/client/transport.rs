use crate::message::AsRawIRC;
use crate::message::IRCMessage;
use crate::message::IRCParseError;
use async_trait::async_trait;
use async_tungstenite::tokio::connect_async;
use bytes::Bytes;
use futures::future::ready;
use futures::prelude::*;
use native_tls::TlsConnector;
use smallvec::SmallVec;
use std::fmt::{Debug, Display};
use thiserror::Error;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio_util::codec::{BytesCodec, FramedWrite};
use tungstenite::Error as WSError;
use tungstenite::Message as WSMessage;
use url::Url;

#[async_trait]
pub trait Transport
where
    Self: Sized + 'static,
{
    type ConnectError: Send + Sync;
    type IncomingError: Send + Sync;
    type OutgoingError: Send + Sync + Debug + Display;

    type Incoming: Stream<Item = Result<IRCMessage, Self::IncomingError>> + Unpin + Send;
    type Outgoing: Sink<IRCMessage, Error = Self::OutgoingError> + Send;

    async fn new() -> Result<Self, Self::ConnectError>;
    fn split(self) -> (Self::Incoming, Self::Outgoing);
}

pub struct TCPTransport {
    incoming_messages: <Self as Transport>::Incoming,
    outgoing_messages: <Self as Transport>::Outgoing,
}

#[derive(Debug, Error)]
pub enum TCPTransportConnectError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    TLSError(#[from] native_tls::Error),
}

#[derive(Debug, Error)]
pub enum TCPTransportIncomingError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    IRCParseError(#[from] IRCParseError),
}

#[async_trait]
impl Transport for TCPTransport {
    type ConnectError = TCPTransportConnectError;
    type IncomingError = TCPTransportIncomingError;
    type OutgoingError = std::io::Error;

    type Incoming = Box<dyn Stream<Item = Result<IRCMessage, Self::IncomingError>> + Unpin + Send>;
    type Outgoing = Box<dyn Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send>;

    async fn new() -> Result<TCPTransport, TCPTransportConnectError> {
        let socket = TcpStream::connect("irc.chat.twitch.tv:6697").await?;
        let cx = TlsConnector::builder().build()?;
        let cx = tokio_tls::TlsConnector::from(cx);
        let socket = cx.connect("irc.chat.twitch.tv", socket).await?;

        let (read_half, write_half) = tokio::io::split(socket);

        let message_stream = BufReader::new(read_half)
            .lines()
            .map_err(TCPTransportIncomingError::from)
            .and_then(|s| ready(IRCMessage::parse(&s).map_err(TCPTransportIncomingError::from)));

        let message_sink =
            FramedWrite::new(write_half, BytesCodec::new()).with(|msg: IRCMessage| {
                let mut s = msg.as_raw_irc();
                s.push_str("\r\n");
                ready(Ok(Bytes::from(s)))
            });

        Ok(TCPTransport {
            incoming_messages: Box::new(message_stream),
            outgoing_messages: Box::new(message_sink),
        })
    }

    fn split(self) -> (Self::Incoming, Self::Outgoing) {
        (self.incoming_messages, self.outgoing_messages)
    }
}

#[derive(Debug, Error)]
pub enum WSTransportIncomingError {
    #[error("{0}")]
    WSError(#[from] WSError),
    #[error("{0}")]
    IRCParseError(#[from] IRCParseError),
}

pub struct WSTransport {
    incoming_messages: <Self as Transport>::Incoming,
    outgoing_messages: <Self as Transport>::Outgoing,
}

#[async_trait]
impl Transport for WSTransport {
    type ConnectError = WSError;
    type IncomingError = WSTransportIncomingError;
    type OutgoingError = WSError;

    type Incoming = Box<dyn Stream<Item = Result<IRCMessage, Self::IncomingError>> + Unpin + Send>;
    type Outgoing = Box<dyn Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send>;

    async fn new() -> Result<WSTransport, tungstenite::error::Error> {
        let (ws_stream, _response) =
            connect_async(Url::parse("wss://irc-ws.chat.twitch.tv").unwrap()).await?;

        let (write_half, read_half) = futures::stream::StreamExt::split(ws_stream);

        let message_stream = read_half
            .map_err(WSTransportIncomingError::from)
            .try_filter_map(|ws_message| {
                ready(Ok::<_, WSTransportIncomingError>(
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
            .and_then(|s| ready(IRCMessage::parse(&s).map_err(WSTransportIncomingError::from)));

        let message_sink =
            write_half.with(|msg: IRCMessage| ready(Ok(WSMessage::Text(msg.as_raw_irc()))));

        Ok(WSTransport {
            incoming_messages: Box::new(message_stream),
            outgoing_messages: Box::new(message_sink),
        })
    }

    fn split(self) -> (Self::Incoming, Self::Outgoing) {
        (self.incoming_messages, self.outgoing_messages)
    }
}
