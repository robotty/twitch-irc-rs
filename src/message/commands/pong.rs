use crate::message::commands::{AsIRCMessage, ServerMessageParseError};
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PongMessage {
    pub argument: Option<String>,
    #[derivative(PartialEq = "ignore")]
    source: Option<IRCMessage>,
}

impl TryFrom<IRCMessage> for PongMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PongMessage, ServerMessageParseError> {
        if source.command != "PONG" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(PongMessage {
            argument: source.params.get(1).cloned(),
            source: Some(source),
        })
    }
}

impl AsIRCMessage for PongMessage {
    fn as_irc_message(&self) -> IRCMessage {
        if let Some(source) = &self.source {
            source.clone()
        } else {
            let params = if let Some(argument) = &self.argument {
                vec![argument.to_owned()]
            } else {
                vec![]
            };

            IRCMessage::new_simple("PONG".to_owned(), params)
        }
    }
}
