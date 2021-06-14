use crate::message::commands::ServerMessageParseError;
use crate::message::commands::ServerMessageParseError::MismatchedCommand;
use crate::message::IRCMessage;
use std::convert::TryFrom;

#[cfg(feature = "serde")]
use {serde::Deserialize, serde::Serialize};

/// Sent by the server to signal a connection to disconnect and reconnect
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReconnectMessage {
    /// The message that this `ReconnectMessage` was parsed from.
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for ReconnectMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<ReconnectMessage, ServerMessageParseError> {
        if source.command == "RECONNECT" {
            Ok(ReconnectMessage { source })
        } else {
            Err(MismatchedCommand(source))
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
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = ReconnectMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            ReconnectMessage {
                source: irc_message
            }
        )
    }
}
