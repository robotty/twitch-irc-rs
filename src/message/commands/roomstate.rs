use crate::message::commands::IRCMessageParseExt;
use crate::message::{IRCMessage, ServerMessageParseError};
use std::convert::TryFrom;
use std::time::Duration;

#[cfg(feature = "serde-commands-support")]
use {
    serde::Deserialize, serde::Serialize
};

/// Sent when a channel is initially joined or when a channel updates it state.
///
/// When a channel is initially is joined, a `ROOMSTATE` message is sent specifying
/// all the settings.
/// If any of these settings are updated while you are joined to a channel,
/// a `ROOMSTATE` is sent only containing the new value for that particular setting.
/// Other settings will be `None`.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-commands-support", derive(Serialize, Deserialize))]
pub struct RoomStateMessage {
    /// Login name of the channel whose "room state" is updated.
    pub channel_login: String,
    /// ID of the channel whose "room state" is updated.
    pub channel_id: String,

    /// If present, specifies a new setting for the "emote only" mode.
    /// (Controlled by `/emoteonly` and `/emoteonlyoff` commands in chat)
    ///
    /// If `true`, emote-only mode was enabled, if `false` emote-only mode was disabled.
    ///
    /// In emote-only mode, users that are not moderator or VIP can only send messages that
    /// are completely composed of emotes.
    pub emote_only: Option<bool>,

    /// If present, specifies a new setting for followers-only mode.
    /// (Controlled by `/followers` and `/followersoff` commands in chat)
    ///
    /// See the documentation on `FollowersOnlyMode` for more details on the possible settings.
    pub follwers_only: Option<FollowersOnlyMode>,

    /// If present, specifies a new setting for the "r9k" beta mode (also sometimes called
    /// unique-chat mode, controlled by the `/r9kbeta` and `/r9kbetaoff` commands)
    ///
    /// If `true`, r9k mode was enabled, if `false` r9k mode was disabled.
    pub r9k: Option<bool>,

    /// If present, specifies a new slow-mode setting. (Controlled by `/slow` and `/slowoff` commands).
    ///
    /// A duration of 0 seconds specifies that slow mode was disabled.
    /// Any non-0 duration specifies the minimum time users must wait between sending individual messages.
    /// Slow-mode does not apply to moderators or VIPs, and in some cases does not apply to subscribers too
    /// (via a setting that the streamer controls).
    ///
    /// Slow mode can only be controlled in increments of full seconds, so this `Duration` will
    /// only contains values that are whole multiples of 1 second.
    pub slow_mode: Option<Duration>,

    /// If present, specifies a new setting for subscribers-only mode (`/subscribers` and
    /// `/subscribersoff` commands).
    ///
    /// If `true`, subscribers-only mode was enabled, if `false`, it was disabled.
    pub subscribers_only: Option<bool>,

    /// The message that this `RoomStateMessage` was parsed from.
    pub source: IRCMessage,
}

/// Specifies the followers-only mode a chat is in or was put in.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-commands-support", derive(Serialize, Deserialize))]
pub enum FollowersOnlyMode {
    /// Followers-only mode is/was disabled. All users, including user that are not followers,
    /// can send chat messages.
    Disabled,

    /// Followers-only mode is/was enabled. All users must have been following for at least this
    /// amount of time before being able to send chat messages.
    ///
    /// Note that this duration can be 0 to signal that all followers can chat. Otherwise,
    /// it will always a value that is a multiple of 1 minute. (1 minute is the highest resolution
    /// that can be specified)
    ///
    /// Moderator, VIPs or
    /// [verified bots](https://dev.twitch.tv/docs/irc/guide#known-and-verified-bots) bypass
    /// this setting and can send messages anyways.
    Enabled(Duration),
}

impl TryFrom<IRCMessage> for RoomStateMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<RoomStateMessage, ServerMessageParseError> {
        if source.command != "ROOMSTATE" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
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
                .map(Duration::from_secs),
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = RoomStateMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.follwers_only,
            Some(FollowersOnlyMode::Enabled(Duration::from_secs(10 * 60))) // 10 minutes
        )
    }

    #[test]
    pub fn test_partial_1() {
        let src = "@room-id=40286300;slow=5 :tmi.twitch.tv ROOMSTATE #randers";
        let irc_message = IRCMessage::parse(src).unwrap();
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
