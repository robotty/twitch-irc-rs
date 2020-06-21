pub mod tcp;
pub mod websocket;

use crate::config::LoginCredentials;
use crate::connection::error::ConnErr;
use crate::message::IRCMessage;
use async_trait::async_trait;
use futures::prelude::*;
use futures::stream::FusedStream;
use std::fmt::{Debug, Display};

#[async_trait]
pub trait Transport<L: LoginCredentials>: Sized + Send + Sync + 'static {
    type ConnectError: Send + Sync + Debug + Display;
    type IncomingError: Send + Sync + Debug + Display;
    type OutgoingError: Send + Sync + Debug + Display + Clone;

    type Incoming: FusedStream<Item = Result<IRCMessage, ConnErr<Self, L>>> + Unpin + Send + Sync;
    type Outgoing: Sink<IRCMessage, Error = Self::OutgoingError> + Unpin + Send + Sync;

    async fn new() -> Result<Self, Self::ConnectError>;
    fn incoming(&mut self) -> &mut Self::Incoming;
    fn outgoing(&mut self) -> &mut Self::Outgoing;
    fn split(self) -> (Self::Incoming, Self::Outgoing);
}
