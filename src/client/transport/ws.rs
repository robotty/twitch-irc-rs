use futures::prelude::*;
use crate::message::IRCMessage;
use super::TransportError;

pub struct WSTransport {
    pub incoming_messages: Box<dyn Stream<Item = Result<IRCMessage, TransportError>>>,
    pub outgoing_messages: Box<dyn Sink<IRCMessage, Error = std::io::Error>>,
}

impl WSTransport {
    pub fn new() -> WSTransport {
        todo!()
    }
}
