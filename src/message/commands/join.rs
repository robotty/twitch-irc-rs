use crate::message::commands::{IRCMessageParseExt, ServerMessageParseError};
use crate::message::IRCMessage;
use std::convert::TryFrom;

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

/// Message received when you successfully join a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct JoinMessage {
    /// Login name of the channel you joined.
    pub channel_login: String,
    /// The login name of the logged in user (the login name of the user that joined the channel,
    /// which is the logged in user).
    pub user_login: String,

    /// The message that this `JoinMessage` was parsed from.
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for JoinMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<JoinMessage, ServerMessageParseError> {
        if source.command != "JOIN" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
        }

        Ok(JoinMessage {
            channel_login: source.try_get_channel_login()?.to_owned(),
            user_login: source.try_get_prefix_nickname()?.to_owned(),
            source,
        })
    }
}

impl From<JoinMessage> for IRCMessage {
    fn from(msg: JoinMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::{IRCMessage, JoinMessage};
    use std::convert::TryFrom;

    #[test]
    pub fn test_basic() {
        let src = ":randers811!randers811@randers811.tmi.twitch.tv JOIN #pajlada";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = JoinMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            JoinMessage {
                channel_login: "pajlada".to_owned(),
                user_login: "randers811".to_owned(),
                source: irc_message
            }
        )
    }
}
