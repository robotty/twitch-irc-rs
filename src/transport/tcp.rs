use crate::message::IRCMessage;
use crate::message::{AsRawIRC, IRCParseError};
use crate::transport::Transport;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{future, sink::Sink, stream::FusedStream, SinkExt, StreamExt, TryStreamExt};
use itertools::Either;
use std::fmt::Debug;
use thiserror::Error;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::net::TcpStream;
use tokio_util::codec::{BytesCodec, FramedWrite};

/// Implements connecting to Twitch chat via a secured (TLS) plain IRC connection.
pub struct TCPTransport {
    incoming_messages: <Self as Transport>::Incoming,
    outgoing_messages: <Self as Transport>::Outgoing,
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TCPTransportConnectError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[cfg(feature = "transport-tcp")]
    #[error("{0}")]
    TLSError(#[from] native_tls::Error),
    #[cfg(feature = "transport-tcp-rustls")]
    #[error("{0}")]
    DnsError(#[from] tokio_rustls::webpki::InvalidDNSNameError),
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
        let socket = TcpStream::connect("irc.chat.twitch.tv:6697").await?;
        let socket = wrap_tls(socket).await?;

        let (read_half, write_half) = tokio::io::split(socket);

        // TODO if tokio re-adds stream support revert to:
        // let message_stream = BufReader::new(read_half)
        //     .lines()
        // then continue with .try_filter() from below
        let mut lines = BufReader::new(read_half).lines();
        let lines_stream = Box::pin(async_stream::stream! {
            while let Some(line) = lines.next_line().await.transpose() {
                yield line;
            }
        });
        let message_stream = lines_stream
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

#[cfg(feature = "transport-tcp")]
async fn wrap_tls(
    socket: TcpStream,
) -> Result<tokio_native_tls::TlsStream<TcpStream>, TCPTransportConnectError> {
    let cx = native_tls::TlsConnector::new().map_err(TCPTransportConnectError::TLSError)?;
    let cx = tokio_native_tls::TlsConnector::from(cx);

    cx.connect("irc.chat.twitch.tv", socket)
        .await
        .map_err(Into::into)
}

#[cfg(all(feature = "transport-tcp-rustls", not(feature = "transport-tcp")))]
async fn wrap_tls(
    socket: TcpStream,
) -> Result<tokio_rustls::client::TlsStream<TcpStream>, TCPTransportConnectError> {
    use std::sync::Arc;
    use tokio_rustls::{rustls::ClientConfig, webpki::DNSNameRef, TlsConnector};

    let mut config = ClientConfig::new();
    #[cfg(feature = "rustls-native-certs")]
    {
        config.root_store = match rustls_native_certs::load_native_certs() {
            Ok(store) => store,
            Err((_, e)) => return Err(e.into()),
        };
    }
    #[cfg(not(feature = "rustls-native-certs"))]
    config
        .root_store
        .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);

    let cx = TlsConnector::from(Arc::new(config));
    let dnsname = DNSNameRef::try_from_ascii_str("irc.chat.twitch.tv")?;

    cx.connect(dnsname, socket).await.map_err(Into::into)
}
