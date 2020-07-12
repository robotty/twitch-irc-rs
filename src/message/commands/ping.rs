use crate::message::commands::ServerMessageParseError;
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PingMessage {
    pub argument: Option<String>,
    #[derivative(PartialEq = "ignore")]
    source: IRCMessage,
}

impl TryFrom<IRCMessage> for PingMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PingMessage, ServerMessageParseError> {
        if source.command != "PING" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(PingMessage {
            argument: source.params.get(1).cloned(),
            source,
        })
    }
}

impl From<PingMessage> for IRCMessage {
    fn from(msg: PingMessage) -> IRCMessage {
        msg.source
    }
}
