use crate::message::IRCMessage;
use crate::message::{AsRawIRC, IRCParseError};
use crate::transport::Transport;
use async_trait::async_trait;
use async_tungstenite::tokio::connect_async;
use futures::prelude::*;
use futures::stream::FusedStream;
use itertools::Either;
use smallvec::SmallVec;
use tungstenite::Error as WSError;
use tungstenite::Message as WSMessage;

/// Implements connecting to Twitch chat via IRC over secure WebSocket.
pub struct WSSTransport {
    incoming_messages: <Self as Transport>::Incoming,
    outgoing_messages: <Self as Transport>::Outgoing,
}

#[async_trait]
impl Transport for WSSTransport {
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

    async fn new() -> Result<WSSTransport, WSError> {
        let (ws_stream, _response) = connect_async("wss://irc-ws.chat.twitch.tv").await?;

        let (write_half, read_half) = futures::stream::StreamExt::split(ws_stream);

        let message_stream = read_half
            .map_err(Either::Left)
            .try_filter_map(|ws_message| {
                future::ready(Ok::<_, Either<WSError, IRCParseError>>(
                    if let WSMessage::Text(text) = ws_message {
                        // the server can send multiple IRC messages in one websocket message,
                        // separated by newlines
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
            // filter empty lines
            .try_filter(|line| future::ready(!line.is_empty()))
            .and_then(|s| future::ready(IRCMessage::parse(&s).map_err(Either::Right)))
            .fuse();

        let message_sink = write_half
            .with(move |msg: IRCMessage| future::ready(Ok(WSMessage::Text(msg.as_raw_irc()))));

        Ok(WSSTransport {
            incoming_messages: Box::new(message_stream),
            outgoing_messages: Box::new(message_sink),
        })
    }

    fn split(self) -> (Self::Incoming, Self::Outgoing) {
        (self.incoming_messages, self.outgoing_messages)
    }
}

impl std::fmt::Debug for WSSTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WSSTransport").finish()
    }
}
