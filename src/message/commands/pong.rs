use crate::message::commands::ServerMessageParseError;
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PongMessage {
    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for PongMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PongMessage, ServerMessageParseError> {
        if source.command != "PONG" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(PongMessage { source })
    }
}

impl From<PongMessage> for IRCMessage {
    fn from(msg: PongMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::{IRCMessage, PongMessage};
    use std::convert::TryFrom;

    #[test]
    pub fn test_basic() {
        // this is what the Twitch servers answers "PING" with
        let src = "PONG :tmi.twitch.tv";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PongMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PongMessage {
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_with_argument() {
        // this is the answer to "PING test"
        let src = ":tmi.twitch.tv PONG tmi.twitch.tv :test";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PongMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PongMessage {
                source: irc_message
            }
        )
    }
}
