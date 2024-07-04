use fast_str::FastStr;

use crate::message::commands::IRCMessageParseExt;
use crate::message::twitch::{Badge, RGBColor};
use crate::message::{IRCMessage, ServerMessageParseError};
use std::collections::HashSet;

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

/// Sent once directly after successful login, containing properties for the logged in user.
///
/// This message is not sent if you log into chat as an anonymous user.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "with-serde",
    derive(
        Serialize,
        Deserialize
    )
)]
pub struct GlobalUserStateMessage {
    /// ID of the logged in user
    pub user_id: FastStr,
    /// Name (also called display name) of the logged in user
    pub user_name: FastStr,
    /// Metadata related to the chat badges in the `badges` tag.
    ///
    /// Currently this is used only for `subscriber`, to indicate the exact number of months
    /// the user has been a subscriber. This number is finer grained than the version number in
    /// badges. For example, a user who has been a subscriber for 45 months would have a
    /// `badge_info` value of 45 but might have a `badges` `version` number for only 3 years.
    ///
    /// However note that subscriber badges are not sent on `GLOBALUSERSTATE` messages,
    /// so you can realistically expect this to be empty unless Twitch adds a new feature.
    pub badge_info: Vec<Badge>,
    /// List of badges the logged in user has in all channels.
    pub badges: Vec<Badge>,
    /// List of emote set IDs the logged in user has available. This always contains at least one entry ("0").
    pub emote_sets: HashSet<FastStr>,
    /// What name color the logged in user has chosen. The same color is used in all channels.
    pub name_color: Option<RGBColor>,

    /// The message that this `GlobalUserStateMessage` was parsed from.
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for GlobalUserStateMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<GlobalUserStateMessage, ServerMessageParseError> {
        if source.command != "GLOBALUSERSTATE" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
        }

        // example:
        // @badge-info=;badges=;color=#19E6E6;display-name=randers;emote-sets=0,42,237,4236,15961,19194,771823,1511293,1641460,1641461,1641462,300206295,300374282,300432482,300548756,472873131,477339272,488737509,537206155,625908879;user-id=40286300;user-type= :tmi.twitch.tv GLOBALUSERSTATE

        Ok(GlobalUserStateMessage {
            user_id: FastStr::from_ref(source.try_get_nonempty_tag_value("user-id")?),
            user_name: FastStr::from_ref(source.try_get_nonempty_tag_value("display-name")?),
            badge_info: source.try_get_badges("badge-info")?,
            badges: source.try_get_badges("badges")?,
            emote_sets: source.try_get_emote_sets("emote-sets")?,
            name_color: source.try_get_color("color")?,
            source,
        })
    }
}

impl From<GlobalUserStateMessage> for IRCMessage {
    fn from(msg: GlobalUserStateMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::twitch::{Badge, RGBColor};
    use crate::message::{GlobalUserStateMessage, IRCMessage};
    use std::collections::HashSet;
    use std::convert::TryFrom;
    use std::iter::FromIterator;

    #[test]
    pub fn test_basic() {
        let src = "@badge-info=;badges=;color=#19E6E6;display-name=randers;emote-sets=0,42,237;user-id=40286300;user-type= :tmi.twitch.tv GLOBALUSERSTATE";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = GlobalUserStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            GlobalUserStateMessage {
                user_id: "40286300".into(),
                user_name: "randers".into(),
                badge_info: vec![],
                badges: vec![],
                emote_sets: vec!["0", "42", "237"]
                    .into_iter()
                    .map(|s| s.into())
                    .collect(),
                name_color: Some(RGBColor {
                    r: 0x19,
                    g: 0xE6,
                    b: 0xE6
                }),
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_badges_no_color() {
        // according to twitch, emote-sets always has 0 in them. I don't trust them though,
        // so this tests that the "empty" case works too.
        let src = "@badge-info=;badges=premium/1;color=;display-name=randers;emote-sets=;user-id=40286300;user-type= :tmi.twitch.tv GLOBALUSERSTATE";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = GlobalUserStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            GlobalUserStateMessage {
                user_id: "40286300".into(),
                user_name: "randers".into(),
                badge_info: vec![],
                badges: vec![Badge {
                    name: "premium".into(),
                    version: "1".into()
                }],
                emote_sets: HashSet::new(),
                name_color: None,
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_plain_new_user() {
        // this is what a freshly registered user gets when logging in
        let src = "@badge-info=;badges=;color=;display-name=randers811;emote-sets=0;user-id=553170741;user-type= :tmi.twitch.tv GLOBALUSERSTATE";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = GlobalUserStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            GlobalUserStateMessage {
                user_id: "553170741".into(),
                user_name: "randers811".into(),
                badge_info: vec![],
                badges: vec![],
                emote_sets: HashSet::from_iter(vec!["0".into()]),
                name_color: None,
                source: irc_message
            }
        )
    }
}
