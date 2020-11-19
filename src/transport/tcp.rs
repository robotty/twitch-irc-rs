use crate::message::IRCMessage;
use crate::message::{AsRawIRC, IRCParseError};
use crate::transport::Transport;
use async_trait::async_trait;
use bytes::Bytes;
use futures::prelude::*;
use futures::stream::FusedStream;
use itertools::Either;
use std::fmt::Debug;
use thiserror::Error;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio::time::Duration;
use tokio_util::codec::{BytesCodec, FramedWrite};

/// Implements connecting to Twitch chat via a secured (TLS) plain IRC connection.
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
    #[error("Connect timed out")]
    TimeoutError,
}

#[async_trait]
impl Transport for TCPTransport {
    type ConnectError = TCPTransportConnectError;
    type IncomingError = std::io::Error;
    type OutgoingError = std::io::Error;

    type Incoming = Box<
        dyn FusedStream<Item = Result<IRCMessage, Either<std::io::Error, IRCParseError>>>
            + Unpin
            + Send
            + Sync,
    >;
    type Outgoing = Box<dyn Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send + Sync>;

    async fn new() -> Result<TCPTransport, TCPTransportConnectError> {
        let connect_attempt = async move {
            let socket = TcpStream::connect("irc.chat.twitch.tv:6697").await?;

            let cx = native_tls::TlsConnector::new().map_err(TCPTransportConnectError::TLSError)?;
            let cx = tokio_native_tls::TlsConnector::from(cx);

            futures::future::pending::<()>().await;
            let socket = cx.connect("irc.chat.twitch.tv", socket).await?;

            let (read_half, write_half) = tokio::io::split(socket);

            let message_stream = BufReader::new(read_half)
                .lines()
                // ignore empty lines
                .try_filter(|line| future::ready(!line.is_empty()))
                .map_err(Either::Left)
                .and_then(|s| future::ready(IRCMessage::parse(&s).map_err(Either::Right)))
                .fuse();

            let message_sink =
                FramedWrite::new(write_half, BytesCodec::new()).with(move |msg: IRCMessage| {
                    let mut s = msg.as_raw_irc();
                    s.push_str("\r\n");
                    future::ready(Ok(Bytes::from(s)))
                });

            Ok(TCPTransport {
                incoming_messages: Box::new(message_stream),
                outgoing_messages: Box::new(message_sink),
            })
        };
        let timeout = tokio::time::delay_for(Duration::from_secs(10));

        tokio::select! {
            res = connect_attempt => {
                res
            }
            _ = timeout => {
                Err(TCPTransportConnectError::TimeoutError)
            }
        }
    }

    fn split(self) -> (Self::Incoming, Self::Outgoing) {
        (self.incoming_messages, self.outgoing_messages)
    }
}

impl std::fmt::Debug for TCPTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TCPTransport").finish()
    }
}
