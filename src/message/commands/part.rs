use crate::message::commands::{IRCMessageParseExt, ServerMessageParseError};
use crate::message::IRCMessage;
use std::convert::TryFrom;

#[cfg(feature = "serde-commands-support")]
use {
    serde::Deserialize, serde::Serialize
};
/// Message received when you successfully leave (part) a channel.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-commands-support", derive(Serialize, Deserialize))]
pub struct PartMessage {
    /// Login name of the channel you parted.
    pub channel_login: String,
    /// The login name of the logged in user (the login name of the user that parted the channel,
    /// which is the logged in user).
    pub user_login: String,
    /// The message that this `PartMessage` was parsed from.
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for PartMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PartMessage, ServerMessageParseError> {
        if source.command != "PART" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
        }

        Ok(PartMessage {
            channel_login: source.try_get_channel_login()?.to_owned(),
            user_login: source.try_get_prefix_nickname()?.to_owned(),
            source,
        })
    }
}

impl From<PartMessage> for IRCMessage {
    fn from(msg: PartMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::{IRCMessage, PartMessage};
    use std::convert::TryFrom;

    #[test]
    pub fn test_basic() {
        let src = ":randers811!randers811@randers811.tmi.twitch.tv PART #pajlada";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PartMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PartMessage {
                channel_login: "pajlada".to_owned(),
                user_login: "randers811".to_owned(),
                source: irc_message
            }
        )
    }
}
