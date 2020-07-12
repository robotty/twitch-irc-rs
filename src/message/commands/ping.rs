use crate::message::commands::ServerMessageParseError;
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PingMessage {
    pub argument: Option<String>,
    #[derivative(PartialEq = "ignore")]
    source: Option<IRCMessage>,
}

impl TryFrom<IRCMessage> for PingMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PingMessage, ServerMessageParseError> {
        if source.command != "PING" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(PingMessage {
            argument: source.params.get(1).cloned(),
            source: Some(source),
        })
    }
}

impl From<PingMessage> for IRCMessage {
    fn from(msg: PingMessage) -> IRCMessage {
        if let Some(source) = msg.source {
            source
        } else {
            let params = if let Some(argument) = msg.argument {
                vec![argument]
            } else {
                vec![]
            };

            IRCMessage::new_simple("PING".to_owned(), params)
        }
    }
}
