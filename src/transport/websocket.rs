use crate::connection::error::ConnectionError;
use crate::login::LoginCredentials;
use crate::message::AsRawIRC;
use crate::message::IRCMessage;
use crate::transport::Transport;
use async_trait::async_trait;
use async_tungstenite::tokio::connect_async;
use futures::future::ready;
use futures::prelude::*;
use futures::stream::FusedStream;
use futures::StreamExt;
use smallvec::SmallVec;
use std::sync::Arc;
use tungstenite::Error as WSError;
use tungstenite::Message as WSMessage;
use url::Url;

pub struct WSSTransport<L: LoginCredentials> {
    incoming_messages: <Self as Transport<L>>::Incoming,
    outgoing_messages: <Self as Transport<L>>::Outgoing,
}

#[async_trait]
impl<L: LoginCredentials> Transport<L> for WSSTransport<L> {
    type ConnectError = WSError;
    type IncomingError = WSError;
    type OutgoingError = Arc<WSError>;

    type Incoming = Box<
        dyn FusedStream<Item = Result<IRCMessage, ConnectionError<Self, L>>> + Unpin + Send + Sync,
    >;
    type Outgoing = Box<dyn Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send + Sync>;

    async fn new() -> Result<WSSTransport<L>, WSError> {
        let (ws_stream, _response) =
            connect_async(Url::parse("wss://irc-ws.chat.twitch.tv").unwrap()).await?;

        let (write_half, read_half) = futures::stream::StreamExt::split(ws_stream);

        let message_stream = read_half
            .map_err(ConnectionError::IncomingError)
            .try_filter_map(|ws_message| {
                ready(Ok::<_, ConnectionError<Self, L>>(
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
            .and_then(|s| ready(IRCMessage::parse(s).map_err(ConnectionError::IRCParseError)));

        let message_sink =
            write_half.with(|msg: IRCMessage| ready(Ok(WSMessage::Text(msg.as_raw_irc()))));

        Ok(WSSTransport {
            incoming_messages: Box::new(message_stream.fuse()),
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
