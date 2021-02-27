use crate::message::IRCMessage;
use crate::message::{AsRawIRC, IRCParseError};
use crate::transport::Transport;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{future, sink::Sink, stream::FusedStream, SinkExt, StreamExt, TryStreamExt};
use itertools::Either;
use std::fmt::Debug;
use thiserror::Error;
use tokio::io::BufReader;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio_stream::wrappers::LinesStream;
use tokio_util::codec::{BytesCodec, FramedWrite};

const TWITCH_SERVER_HOSTNAME: &'static str = "irc.chat.twitch.tv";
const TWITCH_SERVER_PORT_NO_TLS: u16 = 6667;
const TWITCH_SERVER_PORT_TLS: u16 = 6697;

/// Implements connecting to Twitch chat via secured or unsecured plain IRC connection. (ports 6667 or 6697)
pub struct TCPTransport<C: MakeConnection> {
    incoming_messages: <Self as Transport>::Incoming,
    outgoing_messages: <Self as Transport>::Outgoing,
}

/// Errors that can occur while attempting to make a new connection.
///
/// Note that this enum has a different number of variants based on whether the
/// `transport-tcp-native-tls` feature flag is enabled.
#[derive(Debug, Error)]
pub enum TCPTransportConnectError {
    /// Any type of OS-specific I/O error occurred.
    #[error("{0}")]
    IOError(#[from] std::io::Error),

    /// OS-specific error types when using native TLS.
    #[cfg(feature = "transport-tcp-native-tls")]
    #[error("{0}")]
    TLSError(#[from] tokio_native_tls::native_tls::Error),
}

#[async_trait]
pub trait MakeConnection: 'static {
    type Socket: AsyncRead + AsyncWrite + Send + Sync;

    async fn new_socket() -> Result<Self::Socket, TCPTransportConnectError>;
}

#[cfg(any(
    all(
        feature = "transport-tcp-native-tls",
        feature = "transport-tcp-rustls-native-roots"
    ),
    all(
        feature = "transport-tcp-native-tls",
        feature = "transport-tcp-rustls-webpki-roots"
    ),
    all(
        feature = "transport-tcp-rustls-native-roots",
        feature = "transport-tcp-rustls-webpki-roots"
    ),
))]
compile_error!("`transport-tcp-native-tls`, `transport-tcp-rustls-native-roots` and `transport-tcp-rustls-webpki-roots` feature flags are mutually exclusive, enable at most one of them");

pub struct TLS;

#[cfg(feature = "transport-tcp-native-tls")]
#[async_trait]
impl MakeConnection for TLS {
    type Socket = tokio_native_tls::TlsStream<TcpStream>;

    async fn new_socket() -> Result<Self::Socket, TCPTransportConnectError> {
        use tokio_native_tls::native_tls;

        let tcp_socket =
            TcpStream::connect((TWITCH_SERVER_HOSTNAME, TWITCH_SERVER_PORT_TLS)).await?;

        let cx = native_tls::TlsConnector::new()?;
        let cx = tokio_native_tls::TlsConnector::from(cx);

        Ok(cx.connect(TWITCH_SERVER_HOSTNAME, tcp_socket).await?)
    }
}

#[cfg(any(
    feature = "transport-tcp-rustls-native-roots",
    feature = "transport-tcp-rustls-webpki-roots"
))]
#[async_trait]
impl MakeConnection for TLS {
    type Socket = tokio_rustls::client::TlsStream<TcpStream>;

    async fn new_socket() -> Result<Self::Socket, TCPTransportConnectError> {
        use std::sync::Arc;
        use tokio_rustls::{rustls::ClientConfig, webpki::DNSNameRef, TlsConnector};

        let mut config = ClientConfig::new();
        #[cfg(feature = "transport-tcp-rustls-webpki-roots")]
        config
            .root_store
            .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);

        #[cfg(feature = "transport-tcp-rustls-native-roots")]
        {
            config.root_store = match rustls_native_certs::load_native_certs() {
                Ok(cert_store) => cert_store,
                Err((_, e)) => return Err(e.into()),
            };
        }

        let config = TlsConnector::from(Arc::new(config));
        let dnsname = DNSNameRef::try_from_ascii_str(TWITCH_SERVER_HOSTNAME).unwrap();

        let stream = TcpStream::connect((TWITCH_SERVER_HOSTNAME, TWITCH_SERVER_PORT_TLS)).await?;
        Ok(config.connect(dnsname, stream).await?)
    }
}

pub struct NoTLS;

#[async_trait]
impl MakeConnection for NoTLS {
    type Socket = TcpStream;

    async fn new_socket() -> Result<Self::Socket, TCPTransportConnectError> {
        Ok(TcpStream::connect((TWITCH_SERVER_HOSTNAME, TWITCH_SERVER_PORT_NO_TLS)).await?)
    }
}

#[async_trait]
impl<C: MakeConnection> Transport for TCPTransport<C> {
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

    async fn new() -> Result<TCPTransport<C>, TCPTransportConnectError> {
        let socket = C::new_socket().await?;
        let (read_half, write_half) = tokio::io::split(socket);

        // TODO if tokio re-adds stream support revert to:
        // let message_stream = BufReader::new(read_half)
        //     .lines()
        // then continue with .try_filter() from below
        let lines = BufReader::new(read_half).lines();
        let message_stream = LinesStream::new(lines)
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

impl<C: MakeConnection> std::fmt::Debug for TCPTransport<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TCPTransport").finish()
    }
}
