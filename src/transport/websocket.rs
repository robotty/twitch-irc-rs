use crate::message::IRCMessage;
use crate::message::{AsRawIRC, IRCParseError};
use crate::transport::Transport;
use async_trait::async_trait;
use async_tungstenite::tokio::connect_async;
use async_tungstenite::tungstenite::Error as WSError;
use async_tungstenite::tungstenite::Message as WSMessage;
use futures_util::{
    future,
    sink::Sink,
    stream::{self, FusedStream},
    SinkExt, StreamExt, TryStreamExt,
};
use itertools::Either;
use smallvec::SmallVec;

#[cfg(all(
    feature = "transport-ws-native-tls",
    feature = "transport-ws-rustls-webpki-roots"
))]
compile_error!("`transport-ws-native-tls` and `transport-ws-rustls-webpki-roots` feature flags are mutually exclusive, enable at most one of them");

pub trait ConnectionUri: 'static {
    fn get_server_uri() -> &'static str;
}

pub struct TLS;
impl ConnectionUri for TLS {
    fn get_server_uri() -> &'static str {
        "wss://irc-ws.chat.twitch.tv"
    }
}

pub struct NoTLS;
impl ConnectionUri for NoTLS {
    fn get_server_uri() -> &'static str {
        "ws://irc-ws.chat.twitch.tv"
    }
}

/// Implements connecting to Twitch chat via IRC over secure WebSocket.
pub struct WSTransport<C: ConnectionUri> {
    incoming_messages: <Self as Transport>::Incoming,
    outgoing_messages: <Self as Transport>::Outgoing,
}

#[async_trait]
impl<C: ConnectionUri> Transport for WSTransport<C> {
    type ConnectError = WSError;
    type IncomingError = WSError;
    type OutgoingError = WSError;

    type Incoming = Box<
        dyn FusedStream<Item = Result<IRCMessage, Either<WSError, IRCParseError>>>
            + Unpin
            + Send
            + Sync,
    >;
    type Outgoing = Box<dyn Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send + Sync>;

    async fn new() -> Result<WSTransport<C>, WSError> {
        let (ws_stream, _response) = connect_async(C::get_server_uri()).await?;

        let (write_half, read_half) = ws_stream.split();

        let message_stream = read_half
            .map_err(Either::Left)
            .try_filter_map(|ws_message| {
                future::ready(Ok::<_, Either<WSError, IRCParseError>>(
                    if let WSMessage::Text(text) = ws_message {
                        // the server can send multiple IRC messages in one websocket message,
                        // separated by newlines
                        Some(stream::iter(
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
            // filter empty lines
            .try_filter(|line| future::ready(!line.is_empty()))
            .and_then(|s| future::ready(IRCMessage::parse(&s).map_err(Either::Right)))
            .fuse();

        let message_sink = write_half
            .with(move |msg: IRCMessage| future::ready(Ok(WSMessage::Text(msg.as_raw_irc()))));

        Ok(WSTransport {
            incoming_messages: Box::new(message_stream),
            outgoing_messages: Box::new(message_sink),
        })
    }

    fn split(self) -> (Self::Incoming, Self::Outgoing) {
        (self.incoming_messages, self.outgoing_messages)
    }
}

impl<C: ConnectionUri> std::fmt::Debug for WSTransport<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WSSTransport").finish()
    }
}
