//! Implements the different protocols for connecting to Twitch services.

#[cfg(feature = "transport-tcp")]
pub mod tcp;
#[cfg(feature = "transport-ws")]
pub mod websocket;

use crate::message::{IRCMessage, IRCParseError};
use async_trait::async_trait;
use either::Either;
use futures_util::{sink::Sink, stream::FusedStream};
use std::fmt::{Debug, Display};

/// Abstracts over different ways of connecting to Twitch Chat, which are currently
/// plain IRC (TCP), and the Twitch-specific WebSocket extension.
#[async_trait]
pub trait Transport: Sized + Send + Sync + Debug + 'static {
    /// Error type for creating a new connection via `new()`
    type ConnectError: Send + Sync + Debug + Display;
    /// Error type returned from the `Self::Incoming` stream type.
    type IncomingError: Send + Sync + Debug + Display;
    /// Error type returned from the `Self::Outgoing` sink type.
    type OutgoingError: Send + Sync + Debug + Display;

    /// Type of stream of incoming messages.
    type Incoming: FusedStream<Item = Result<IRCMessage, Either<Self::IncomingError, IRCParseError>>>
        + Unpin
        + Send
        + Sync;
    /// Type of outgoing messages sink.
    type Outgoing: Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send + Sync;

    /// Try to create and connect a new `Transport` of this type. Returns `Ok(Self)` after
    /// the connection was established successfully.
    async fn new() -> Result<Self, Self::ConnectError>;
    /// Split this transport into its incoming and outgoing halves (streams).
    fn split(self) -> (Self::Incoming, Self::Outgoing);
}
