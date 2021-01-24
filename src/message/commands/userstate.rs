use crate::message::commands::IRCMessageParseExt;
use crate::message::twitch::{Badge, RGBColor};
use crate::message::{IRCMessage, ServerMessageParseError};
use std::collections::HashSet;
use std::convert::TryFrom;

/// Sent when you join a channel or when you successfully sent a `PRIVMSG` message to a channel.
///
/// This specifies details about the logged in user in a given channel.
///
/// This message is similar to `GLOBALUSERSTATE`, but carries the context of a `channel_login`
/// (and therefore possibly different `badges` and `badge_info`) and omits the `user_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct UserStateMessage {
    /// Login name of the channel this `USERSTATE` message specifies the logged in user's state in.
    pub channel_login: String,
    /// (Display) name of the logged in user.
    pub user_name: String,
    /// Metadata related to the chat badges in the `badges` tag.
    ///
    /// Currently this is used only for `subscriber`, to indicate the exact number of months
    /// the user has been a subscriber. This number is finer grained than the version number in
    /// badges. For example, a user who has been a subscriber for 45 months would have a
    /// `badge_info` value of 45 but might have a `badges` `version` number for only 3 years.
    pub badge_info: Vec<Badge>,
    /// List of badges the logged in user has in this channel.
    pub badges: Vec<Badge>,
    /// List of emote set IDs the logged in user has available. This always contains at least 0.
    pub emote_sets: HashSet<u64>,
    /// What name color the logged in user has chosen. The same color is used in all channels.
    pub name_color: Option<RGBColor>,

    /// The message that this `UserStateMessage` was parsed from.
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for UserStateMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<UserStateMessage, ServerMessageParseError> {
        if source.command != "USERSTATE" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
        }

        Ok(UserStateMessage {
            channel_login: source.try_get_channel_login()?.to_owned(),
            user_name: source
                .try_get_nonempty_tag_value("display-name")?
                .to_owned(),
            badge_info: source.try_get_badges("badge-info")?,
            badges: source.try_get_badges("badges")?,
            emote_sets: source.try_get_emote_sets("emote-sets")?,
            name_color: source.try_get_color("color")?,
            source,
        })
    }
}

impl From<UserStateMessage> for IRCMessage {
    fn from(msg: UserStateMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::commands::userstate::UserStateMessage;
    use crate::message::twitch::RGBColor;
    use crate::message::IRCMessage;
    use std::convert::TryFrom;

    #[test]
    pub fn test_basic() {
        let src = "@badge-info=;badges=;color=#FF0000;display-name=TESTUSER;emote-sets=0;mod=0;subscriber=0;user-type= :tmi.twitch.tv USERSTATE #randers";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            UserStateMessage {
                channel_login: "randers".to_owned(),
                user_name: "TESTUSER".to_owned(),
                badge_info: vec![],
                badges: vec![],
                emote_sets: vec![0].into_iter().collect(),
                name_color: Some(RGBColor {
                    r: 0xFF,
                    g: 0x00,
                    b: 0x00
                }),
                source: irc_message
            }
        )
    }
}
