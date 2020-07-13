use crate::message::commands::ServerMessageParseError;
use crate::message::commands::ServerMessageParseError::MismatchedCommand;
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct ReconnectMessage {
    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for ReconnectMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<ReconnectMessage, ServerMessageParseError> {
        if source.command == "RECONNECT" {
            Ok(ReconnectMessage { source })
        } else {
            Err(MismatchedCommand())
        }
    }
}

impl From<ReconnectMessage> for IRCMessage {
    fn from(msg: ReconnectMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::{IRCMessage, ReconnectMessage};
    use std::convert::TryFrom;

    #[test]
    pub fn test_basic() {
        let src = ":tmi.twitch.tv RECONNECT";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = ReconnectMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            ReconnectMessage {
                source: irc_message
            }
        )
    }
}
