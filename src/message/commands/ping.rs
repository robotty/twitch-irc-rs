use crate::message::commands::{AsIRCMessage, ServerMessageParseError};
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

impl AsIRCMessage for PingMessage {
    fn as_irc_message(&self) -> IRCMessage {
        if let Some(source) = &self.source {
            source.clone()
        } else {
            let params = if let Some(argument) = &self.argument {
                vec![argument.to_owned()]
            } else {
                vec![]
            };

            IRCMessage::new_simple("PING".to_owned(), params)
        }
    }
}
