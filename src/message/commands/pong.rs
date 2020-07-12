use crate::message::commands::ServerMessageParseError;
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PongMessage {
    pub argument: Option<String>,
    #[derivative(PartialEq = "ignore")]
    source: IRCMessage,
}

impl TryFrom<IRCMessage> for PongMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PongMessage, ServerMessageParseError> {
        if source.command != "PONG" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(PongMessage {
            argument: source.params.get(1).cloned(),
            source,
        })
    }
}

impl From<PongMessage> for IRCMessage {
    fn from(msg: PongMessage) -> IRCMessage {
        msg.source
    }
}
