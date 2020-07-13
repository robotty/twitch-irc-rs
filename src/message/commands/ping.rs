use crate::message::commands::ServerMessageParseError;
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PingMessage {
    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for PingMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PingMessage, ServerMessageParseError> {
        if source.command != "PING" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(PingMessage { source })
    }
}

impl From<PingMessage> for IRCMessage {
    fn from(msg: PingMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::{IRCMessage, PingMessage};
    use std::convert::TryFrom;

    #[test]
    pub fn test_basic() {
        let src = ":tmi.twitch.tv PING";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PingMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PingMessage {
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_with_arguments() {
        // want to make sure we can handle changing formats
        let src = ":tmi.twitch.tv PING test :abc def";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PingMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PingMessage {
                source: irc_message
            }
        )
    }
}
