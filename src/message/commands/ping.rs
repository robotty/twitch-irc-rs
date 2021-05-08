use crate::message::commands::ServerMessageParseError;
use crate::message::IRCMessage;
use std::convert::TryFrom;

#[cfg(feature = "serde-commands-support")]
use {serde::Deserialize, serde::Serialize};

/// A `PING` connection-control message.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-commands-support", derive(Serialize, Deserialize))]
pub struct PingMessage {
    /// The message that this `PingMessage` was parsed from.
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for PingMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PingMessage, ServerMessageParseError> {
        if source.command != "PING" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PingMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PingMessage {
                source: irc_message
            }
        )
    }
}
