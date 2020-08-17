use crate::message::IRCMessage;
use crate::message::{AsRawIRC, IRCParseError};
use crate::transport::Transport;
use async_trait::async_trait;
use bytes::Bytes;
use futures::prelude::*;
use futures::stream::FusedStream;
use itertools::Either;
use std::borrow::Cow;
use std::fmt::Debug;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio_util::codec::{BytesCodec, FramedWrite};

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

#[async_trait]
impl Transport for TCPTransport {
    type ConnectError = TCPTransportConnectError;
    type IncomingError = std::io::Error;
    type OutgoingError = Arc<std::io::Error>;

    type Incoming = Box<
        dyn FusedStream<Item = Result<IRCMessage, Either<std::io::Error, IRCParseError>>>
            + Unpin
            + Send
            + Sync,
    >;
    type Outgoing = Box<dyn Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send + Sync>;

    async fn new(
        metrics_identifier: Option<Cow<'static, str>>,
    ) -> Result<TCPTransport, TCPTransportConnectError> {
        let socket = TcpStream::connect("irc.chat.twitch.tv:6697").await?;

        let cx = native_tls::TlsConnector::new().map_err(TCPTransportConnectError::TLSError)?;
        let cx = tokio_native_tls::TlsConnector::from(cx);

        let socket = cx.connect("irc.chat.twitch.tv", socket).await?;

        let (read_half, write_half) = tokio::io::split(socket);

        let metrics_identifier_clone = metrics_identifier.clone();
        let message_stream = BufReader::new(read_half)
            .lines()
            // ignore empty lines
            .try_filter(|line| future::ready(!line.is_empty()))
            .map_err(Either::Left)
            .and_then(|s| future::ready(IRCMessage::parse(&s).map_err(Either::Right)))
            .inspect_ok(move |msg| {
                log::trace!("< {}", msg.as_raw_irc());
                if let Some(ref metrics_identifier) = metrics_identifier_clone {
                    metrics::counter!(
                        "twitch_irc_messages_received",
                        1,
                        "client" => metrics_identifier.clone(),
                        "command" => msg.command.clone()
                    )
                }
            })
            .fuse();

        let message_sink =
            FramedWrite::new(write_half, BytesCodec::new()).with(move |msg: IRCMessage| {
                log::trace!("> {}", msg.as_raw_irc());
                if let Some(ref metrics_identifier) = metrics_identifier {
                    metrics::counter!(
                        "twitch_irc_messages_sent",
                        1,
                        "client" => metrics_identifier.clone(),
                        "command" => msg.command.clone()
                    )
                }

                let mut s = msg.as_raw_irc();
                s.push_str("\r\n");
                future::ready(Ok(Bytes::from(s)))
            });

        Ok(TCPTransport {
            incoming_messages: Box::new(message_stream),
            outgoing_messages: Box::new(message_sink),
        })
    }

    fn incoming(&mut self) -> &mut Self::Incoming {
        &mut self.incoming_messages
    }

    fn outgoing(&mut self) -> &mut Self::Outgoing {
        &mut self.outgoing_messages
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
