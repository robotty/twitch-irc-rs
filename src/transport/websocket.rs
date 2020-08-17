use crate::message::IRCMessage;
use crate::message::{AsRawIRC, IRCParseError};
use crate::transport::Transport;
use async_trait::async_trait;
use async_tungstenite::tokio::connect_async;
use futures::prelude::*;
use futures::stream::FusedStream;
use itertools::Either;
use smallvec::SmallVec;
use std::borrow::Cow;
use std::sync::Arc;
use tungstenite::Error as WSError;
use tungstenite::Message as WSMessage;

pub struct WSSTransport {
    incoming_messages: <Self as Transport>::Incoming,
    outgoing_messages: <Self as Transport>::Outgoing,
}

#[async_trait]
impl Transport for WSSTransport {
    type ConnectError = WSError;
    type IncomingError = WSError;
    type OutgoingError = Arc<WSError>;

    type Incoming = Box<
        dyn FusedStream<Item = Result<IRCMessage, Either<WSError, IRCParseError>>>
            + Unpin
            + Send
            + Sync,
    >;
    type Outgoing = Box<dyn Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send + Sync>;

    async fn new(metrics_identifier: Option<Cow<'static, str>>) -> Result<WSSTransport, WSError> {
        let (ws_stream, _response) = connect_async("wss://irc-ws.chat.twitch.tv").await?;

        let (write_half, read_half) = futures::stream::StreamExt::split(ws_stream);

        let metrics_identifier_clone = metrics_identifier.clone();
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

        let message_sink = write_half.with(move |msg: IRCMessage| {
            log::trace!("> {}", msg.as_raw_irc());
            if let Some(ref metrics_identifier) = metrics_identifier {
                metrics::counter!(
                    "twitch_irc_messages_sent",
                    1,
                    "client" => metrics_identifier.clone(),
                    "command" => msg.command.clone()
                )
            }

            future::ready(Ok(WSMessage::Text(msg.as_raw_irc())))
        });

        Ok(WSSTransport {
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

impl std::fmt::Debug for WSSTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WSSTransport").finish()
    }
}
