use fast_str::FastStr;

use crate::message::commands::{IRCMessageParseExt, ServerMessageParseError};
use crate::message::IRCMessage;

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

/// Message received when you successfully leave (part) a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "with-serde",
    derive(
        Serialize,
        Deserialize
    )
)]
pub struct PartMessage {
    /// Login name of the channel you parted.
    pub channel_login: FastStr,
    /// The login name of the logged in user (the login name of the user that parted the channel,
    /// which is the logged in user).
    pub user_login: FastStr,
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
            channel_login: FastStr::from_ref(source.try_get_channel_login()?),
            user_login: FastStr::from_ref(source.try_get_prefix_nickname()?),
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
                channel_login: "pajlada".into(),
                user_login: "randers811".into(),
                source: irc_message
            }
        )
    }
}
