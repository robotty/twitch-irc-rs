use super::Transport;
use super::TransportError;
use crate::message::IRCMessage;
use futures::prelude::*;

pub struct WSTransport {
    pub incoming_messages: Box<dyn Stream<Item = Result<IRCMessage, TransportError>>>,
    pub outgoing_messages: Box<dyn Sink<IRCMessage, Error = std::io::Error>>,
}

impl WSTransport {
    pub fn new() -> WSTransport {
        todo!()
    }
}

impl Transport for WSTransport {
    fn split(
        self,
    ) -> (
        Box<dyn Stream<Item = Result<IRCMessage, TransportError>>>,
        Box<dyn Sink<IRCMessage, Error = std::io::Error>>,
    ) {
        (self.incoming_messages, self.outgoing_messages)
    }
}
