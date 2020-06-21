use crate::config::LoginCredentials;
use crate::connection::error::ConnErr;
use crate::message::AsRawIRC;
use crate::message::IRCMessage;
use crate::transport::Transport;
use async_trait::async_trait;
use bytes::Bytes;
use futures::prelude::*;
use futures::stream::FusedStream;
use native_tls;
use std::fmt::Debug;
use std::sync::Arc;
use thiserror::Error;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio::prelude::*;
use tokio_util::codec::{BytesCodec, FramedWrite};

pub struct TCPTransport<L: LoginCredentials> {
    incoming_messages: <Self as Transport<L>>::Incoming,
    outgoing_messages: <Self as Transport<L>>::Outgoing,
}

#[derive(Debug, Error)]
pub enum TCPTransportConnectError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    TLSError(#[from] native_tls::Error),
}

#[async_trait]
impl<L: LoginCredentials> Transport<L> for TCPTransport<L> {
    type ConnectError = TCPTransportConnectError;
    type IncomingError = std::io::Error;
    type OutgoingError = Arc<std::io::Error>;

    type Incoming =
        Box<dyn FusedStream<Item = Result<IRCMessage, ConnErr<Self, L>>> + Unpin + Send + Sync>;
    type Outgoing = Box<dyn Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send + Sync>;

    async fn new() -> Result<TCPTransport<L>, TCPTransportConnectError> {
        let socket = TcpStream::connect("irc.chat.twitch.tv:6697").await?;

        // let cx = native_tls::TlsConnector::new().map_err(TCPTransportConnectError::TLSError)?;
        // let cx = tokio_native_tls::TlsConnector::from(cx);
        let cx = native_tls::TlsConnector::new().map_err(TCPTransportConnectError::TLSError)?;
        let cx = tokio_native_tls::TlsConnector::from(cx);

        let socket = cx.connect("irc.chat.twitch.tv", socket).await?;

        let (read_half, write_half) = tokio::io::split(socket);

        let message_stream = BufReader::new(read_half)
            .lines()
            .map_err(ConnErr::<Self, L>::IncomingError)
            .and_then(|s| {
                future::ready(IRCMessage::parse(s).map_err(ConnErr::<Self, L>::IRCParseError))
            })
            .fuse();

        let message_sink =
            FramedWrite::new(write_half, BytesCodec::new()).with(|msg: IRCMessage| {
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
