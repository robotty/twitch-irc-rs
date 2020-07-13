use crate::message::commands::IRCMessageParseExt;
use crate::message::{IRCMessage, ServerMessageParseError};
use derivative::Derivative;
use std::convert::TryFrom;
use std::time::Duration;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct RoomStateMessage {
    pub channel_login: String,
    pub channel_id: String,

    pub emote_only: Option<bool>,
    pub follwers_only: Option<FollowersOnlyMode>,
    pub r9k: Option<bool>,
    pub slow_mode: Option<Duration>,
    pub subscribers_only: Option<bool>,

    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FollowersOnlyMode {
    Disabled,
    Enabled(Duration),
}

impl TryFrom<IRCMessage> for RoomStateMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<RoomStateMessage, ServerMessageParseError> {
        if source.command != "ROOMSTATE" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        // examples:
        // full state: @emote-only=0;followers-only=-1;r9k=0;rituals=0;room-id=40286300;slow=0;subs-only=0 :tmi.twitch.tv ROOMSTATE #randers
        // just one of the properties was updated: @emote-only=1;room-id=40286300 :tmi.twitch.tv ROOMSTATE #randers

        // emote-only, r9k, subs-only: 0 (disabled) or 1 (enabled).
        // followers-only: -1 means disabled, 0 means all followers can chat (essentially
        // duration = 0), and any number above 0 is the time in minutes before user can take)
        // slow: number of seconds between messages that users have to wait. Disabled slow-mode
        // is slow=0, anything other than that is enabled

        Ok(RoomStateMessage {
            channel_login: source.try_get_channel_login()?.to_owned(),
            channel_id: source.try_get_nonempty_tag_value("room-id")?.to_owned(),
            emote_only: source.try_get_optional_bool("emote-only")?,
            follwers_only: source
                .try_get_optional_number::<i64>("followers-only")?
                .map(|n| match n {
                    n if n >= 0 => FollowersOnlyMode::Enabled(Duration::from_secs((n * 60) as u64)),
                    _ => FollowersOnlyMode::Disabled,
                }),
            r9k: source.try_get_optional_bool("r9k")?,
            slow_mode: source
                .try_get_optional_number::<u64>("slow")?
                .map(|n| Duration::from_secs(n)),
            subscribers_only: source.try_get_optional_bool("subs-only")?,
            source,
        })
    }
}

impl From<RoomStateMessage> for IRCMessage {
    fn from(msg: RoomStateMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::commands::roomstate::FollowersOnlyMode;
    use crate::message::{IRCMessage, RoomStateMessage};
    use std::convert::TryFrom;
    use std::time::Duration;

    #[test]
    pub fn test_basic_full() {
        let src = "@emote-only=0;followers-only=-1;r9k=0;rituals=0;room-id=40286300;slow=0;subs-only=0 :tmi.twitch.tv ROOMSTATE #randers";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = RoomStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            RoomStateMessage {
                channel_login: "randers".to_owned(),
                channel_id: "40286300".to_owned(),
                emote_only: Some(false),
                follwers_only: Some(FollowersOnlyMode::Disabled),
                r9k: Some(false),
                slow_mode: Some(Duration::from_secs(0)),
                subscribers_only: Some(false),
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_basic_full2() {
        let src = "@emote-only=1;followers-only=0;r9k=1;rituals=0;room-id=40286300;slow=5;subs-only=1 :tmi.twitch.tv ROOMSTATE #randers";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = RoomStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            RoomStateMessage {
                channel_login: "randers".to_owned(),
                channel_id: "40286300".to_owned(),
                emote_only: Some(true),
                follwers_only: Some(FollowersOnlyMode::Enabled(Duration::from_secs(0))),
                r9k: Some(true),
                slow_mode: Some(Duration::from_secs(5)),
                subscribers_only: Some(true),
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_followers_non_zero() {
        let src = "@emote-only=1;followers-only=10;r9k=1;rituals=0;room-id=40286300;slow=5;subs-only=1 :tmi.twitch.tv ROOMSTATE #randers";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = RoomStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg.follwers_only,
            Some(FollowersOnlyMode::Enabled(Duration::from_secs(10 * 60))) // 10 minutes
        )
    }

    #[test]
    pub fn test_partial_1() {
        let src = "@room-id=40286300;slow=5 :tmi.twitch.tv ROOMSTATE #randers";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = RoomStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            RoomStateMessage {
                channel_login: "randers".to_owned(),
                channel_id: "40286300".to_owned(),
                emote_only: None,
                follwers_only: None,
                r9k: None,
                slow_mode: Some(Duration::from_secs(5)),
                subscribers_only: None,
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_partial_2() {
        let src = "@emote-only=1;room-id=40286300 :tmi.twitch.tv ROOMSTATE #randers";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = RoomStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            RoomStateMessage {
                channel_login: "randers".to_owned(),
                channel_id: "40286300".to_owned(),
                emote_only: Some(true),
                follwers_only: None,
                r9k: None,
                slow_mode: None,
                subscribers_only: None,
                source: irc_message
            }
        )
    }
}
