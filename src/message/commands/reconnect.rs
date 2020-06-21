use crate::message::commands::ServerMessageParseError::MismatchedCommand;
use crate::message::commands::{AsIRCMessage, ServerMessageParseError};
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct ReconnectMessage {
    #[derivative(PartialEq = "ignore")]
    source: Option<IRCMessage>,
}

impl TryFrom<IRCMessage> for ReconnectMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<ReconnectMessage, ServerMessageParseError> {
        if source.command == "RECONNECT" {
            Ok(ReconnectMessage {
                source: Some(source),
            })
        } else {
            Err(MismatchedCommand())
        }
    }
}

impl AsIRCMessage for ReconnectMessage {
    fn as_irc_message(&self) -> IRCMessage {
        if let Some(source) = &self.source {
            source.clone()
        } else {
            IRCMessage::new_simple("RECONNECT".to_owned(), vec![])
        }
    }
}
