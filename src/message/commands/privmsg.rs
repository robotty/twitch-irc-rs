use crate::message::commands::IRCMessageParseExt;
use crate::message::twitch::{Badge, Emote, RGBColor, TwitchUserBasics};
use crate::message::{IRCMessage, ReplyParent, ReplyToMessage, ServerMessageParseError};
use chrono::{DateTime, Utc};
use fast_str::FastStr;

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

/// A regular Twitch chat message.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "with-serde",
    derive(
        Serialize,
        Deserialize
    )
)]
pub struct PrivmsgMessage {
    /// Login name of the channel that the message was sent to.
    pub channel_login: FastStr,
    /// ID of the channel that the message was sent to.
    pub channel_id: FastStr,
    /// The message text that was sent.
    pub message_text: FastStr,
    /// Optional reply parent of the message, containing data about the message that this message is replying to.
    pub reply_parent: Option<ReplyParent>,
    /// Whether this message was made using the `/me` command.
    ///
    /// These type of messages are typically fully colored with `name_color` and
    /// have no `:` separating the sending user and the message.
    ///
    /// The `message_text` does not contain the `/me` command or the control sequence
    /// (`\x01ACTION <msg>\x01`) that is used for these action messages.
    pub is_action: bool,
    /// The user that sent this message.
    pub sender: TwitchUserBasics,
    /// Metadata related to the chat badges in the `badges` tag.
    ///
    /// Currently this is used only for `subscriber`, to indicate the exact number of months
    /// the user has been a subscriber. This number is finer grained than the version number in
    /// badges. For example, a user who has been a subscriber for 45 months would have a
    /// `badge_info` value of 45 but might have a `badges` `version` number for only 3 years.
    pub badge_info: Vec<Badge>,
    /// List of badges that should be displayed alongside the message.
    pub badges: Vec<Badge>,
    /// If present, specifies how many bits were cheered with this message.
    pub bits: Option<u64>,
    /// If present, specifies the color that the user's name should be displayed in. A value
    /// of `None` here signifies that the user has not picked any particular color.
    /// Implementations differ on how they handle this, on the Twitch website users are assigned
    /// a pseudorandom but consistent-per-user color if they have no color specified.
    pub name_color: Option<RGBColor>,
    /// A list of emotes in this message. Each emote replaces a part of the `message_text`.
    /// These emotes are sorted in the order that they appear in the message.
    pub emotes: Vec<Emote>,
    /// A FastStr uniquely identifying this message. Can be used with the Twitch API to
    /// delete single messages. See also the `CLEARMSG` message type.
    pub message_id: FastStr,
    /// Timestamp of when this message was sent.
    pub server_timestamp: DateTime<Utc>,

    /// The message that this `PrivmsgMessage` was parsed from.
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for PrivmsgMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PrivmsgMessage, ServerMessageParseError> {
        if source.command != "PRIVMSG" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
        }

        let (message_text, is_action) = source.try_get_message_text()?;

        Ok(PrivmsgMessage {
            channel_login: FastStr::from_ref(source.try_get_channel_login()?),
            channel_id: FastStr::from_ref(source.try_get_nonempty_tag_value("room-id")?),
            sender: TwitchUserBasics {
                id: FastStr::from_ref(source.try_get_nonempty_tag_value("user-id")?),
                login: FastStr::from_ref(source.try_get_prefix_nickname()?),
                name: FastStr::from_ref(source.try_get_nonempty_tag_value("display-name")?),
            },
            badge_info: source.try_get_badges("badge-info")?,
            badges: source.try_get_badges("badges")?,
            bits: source.try_get_optional_number("bits")?,
            name_color: source.try_get_color("color")?,
            emotes: source.try_get_emotes("emotes", message_text)?,
            server_timestamp: source.try_get_timestamp("tmi-sent-ts")?,
            message_id: FastStr::from_ref(source.try_get_nonempty_tag_value("id")?),
            message_text: FastStr::from_ref(message_text),
            reply_parent: source.try_get_optional_reply_parent()?,
            is_action,
            source,
        })
    }
}

impl From<PrivmsgMessage> for IRCMessage {
    fn from(msg: PrivmsgMessage) -> IRCMessage {
        msg.source
    }
}

impl ReplyToMessage for PrivmsgMessage {
    fn channel_login(&self) -> &str {
        &self.channel_login
    }

    fn message_id(&self) -> &str {
        &self.message_id
    }
}

#[cfg(test)]
mod tests {
    use crate::message::twitch::{Badge, Emote, RGBColor, TwitchUserBasics};
    use crate::message::{IRCMessage, PrivmsgMessage, ReplyParent};
    use chrono::offset::TimeZone;
    use chrono::Utc;
    use std::convert::TryFrom;
    use std::ops::Range;

    #[test]
    fn test_basic_example() {
        let src = "@badge-info=;badges=;color=#0000FF;display-name=JuN1oRRRR;emotes=;flags=;id=e9d998c3-36f1-430f-89ec-6b887c28af36;mod=0;room-id=11148817;subscriber=0;tmi-sent-ts=1594545155039;turbo=0;user-id=29803735;user-type= :jun1orrrr!jun1orrrr@jun1orrrr.tmi.twitch.tv PRIVMSG #pajlada :dank cam";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PrivmsgMessage {
                channel_login: "pajlada".to_owned(),
                channel_id: "11148817".to_owned(),
                message_text: "dank cam".to_owned(),
                is_action: false,
                sender: TwitchUserBasics {
                    id: "29803735".to_owned(),
                    login: "jun1orrrr".to_owned(),
                    name: "JuN1oRRRR".to_owned()
                },
                badge_info: vec![],
                badges: vec![],
                bits: None,
                name_color: Some(RGBColor {
                    r: 0x00,
                    g: 0x00,
                    b: 0xFF
                }),
                emotes: vec![],
                server_timestamp: Utc.timestamp_millis_opt(1594545155039).unwrap(),
                message_id: "e9d998c3-36f1-430f-89ec-6b887c28af36".to_owned(),
                reply_parent: None,

                source: irc_message
            }
        );
    }

    #[test]
    fn test_action_and_badges() {
        let src = "@badge-info=subscriber/22;badges=moderator/1,subscriber/12;color=#19E6E6;display-name=randers;emotes=;flags=;id=d831d848-b7c7-4559-ae3a-2cb88f4dbfed;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1594555275886;turbo=0;user-id=40286300;user-type=mod :randers!randers@randers.tmi.twitch.tv PRIVMSG #pajlada :ACTION -tags";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PrivmsgMessage {
                channel_login: "pajlada".to_owned(),
                channel_id: "11148817".to_owned(),
                message_text: "-tags".to_owned(),
                is_action: true,
                sender: TwitchUserBasics {
                    id: "40286300".to_owned(),
                    login: "randers".to_owned(),
                    name: "randers".to_owned()
                },
                badge_info: vec![Badge {
                    name: "subscriber".to_owned(),
                    version: "22".to_owned()
                }],
                badges: vec![
                    Badge {
                        name: "moderator".to_owned(),
                        version: "1".to_owned()
                    },
                    Badge {
                        name: "subscriber".to_owned(),
                        version: "12".to_owned()
                    }
                ],
                bits: None,
                name_color: Some(RGBColor {
                    r: 0x19,
                    g: 0xE6,
                    b: 0xE6
                }),
                emotes: vec![],
                server_timestamp: Utc.timestamp_millis_opt(1594555275886).unwrap(),
                message_id: "d831d848-b7c7-4559-ae3a-2cb88f4dbfed".to_owned(),
                reply_parent: None,
                source: irc_message
            }
        );
    }

    #[test]
    fn test_greyname_no_color() {
        let src = "@rm-received-ts=1594554085918;historical=1;badge-info=;badges=;client-nonce=815810609edecdf4537bd9586994182b;color=;display-name=CarvedTaleare;emotes=;flags=;id=c9b941d9-a0ab-4534-9903-971768fcdf10;mod=0;room-id=22484632;subscriber=0;tmi-sent-ts=1594554085753;turbo=0;user-id=467684514;user-type= :carvedtaleare!carvedtaleare@carvedtaleare.tmi.twitch.tv PRIVMSG #forsen :NaM";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PrivmsgMessage {
                channel_login: "forsen".to_owned(),
                channel_id: "22484632".to_owned(),
                message_text: "NaM".to_owned(),
                is_action: false,
                sender: TwitchUserBasics {
                    id: "467684514".to_owned(),
                    login: "carvedtaleare".to_owned(),
                    name: "CarvedTaleare".to_owned()
                },
                badge_info: vec![],
                badges: vec![],
                bits: None,
                name_color: None,
                emotes: vec![],
                server_timestamp: Utc.timestamp_millis_opt(1594554085753).unwrap(),
                message_id: "c9b941d9-a0ab-4534-9903-971768fcdf10".to_owned(),
                reply_parent: None,

                source: irc_message
            }
        );
    }

    #[test]
    fn test_reply_parent_included() {
        let src = "@badge-info=;badges=;client-nonce=cd56193132f934ac71b4d5ac488d4bd6;color=;display-name=LeftSwing;emotes=;first-msg=0;flags=;id=5b4f63a9-776f-4fce-bf3c-d9707f52e32d;mod=0;reply-parent-display-name=Retoon;reply-parent-msg-body=hello;reply-parent-msg-id=6b13e51b-7ecb-43b5-ba5b-2bb5288df696;reply-parent-user-id=37940952;reply-parent-user-login=retoon;returning-chatter=0;room-id=37940952;subscriber=0;tmi-sent-ts=1673925983585;turbo=0;user-id=133651738;user-type= :leftswing!leftswing@leftswing.tmi.twitch.tv PRIVMSG #retoon :@Retoon yes";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PrivmsgMessage {
                channel_login: "retoon".to_owned(),
                channel_id: "37940952".to_owned(),
                message_text: "@Retoon yes".to_owned(),
                is_action: false,
                sender: TwitchUserBasics {
                    id: "133651738".to_owned(),
                    login: "leftswing".to_owned(),
                    name: "LeftSwing".to_owned()
                },
                badge_info: vec![],
                badges: vec![],
                bits: None,
                name_color: None,
                emotes: vec![],
                server_timestamp: Utc.timestamp_millis_opt(1673925983585).unwrap(),
                message_id: "5b4f63a9-776f-4fce-bf3c-d9707f52e32d".to_owned(),
                reply_parent: Some(ReplyParent {
                    message_id: "6b13e51b-7ecb-43b5-ba5b-2bb5288df696".to_owned(),
                    reply_parent_user: TwitchUserBasics {
                        id: "37940952".to_owned(),
                        login: "retoon".to_FastStr(),
                        name: "Retoon".to_owned(),
                    },
                    message_text: "hello".to_owned()
                }),

                source: irc_message
            }
        );
    }

    #[test]
    fn test_display_name_with_trailing_space() {
        let src = "@rm-received-ts=1594554085918;historical=1;badge-info=;badges=;client-nonce=815810609edecdf4537bd9586994182b;color=;display-name=CarvedTaleare\\s;emotes=;flags=;id=c9b941d9-a0ab-4534-9903-971768fcdf10;mod=0;room-id=22484632;subscriber=0;tmi-sent-ts=1594554085753;turbo=0;user-id=467684514;user-type= :carvedtaleare!carvedtaleare@carvedtaleare.tmi.twitch.tv PRIVMSG #forsen :NaM";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();
        assert_eq!(msg.sender.name, "CarvedTaleare ");
    }

    #[test]
    fn test_korean_display_name() {
        let src = "@badge-info=subscriber/35;badges=moderator/1,subscriber/3024;color=#FF0000;display-name=테스트계정420;emotes=;flags=;id=bdfa278e-11c4-484f-9491-0a61b16fab60;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1593953876927;turbo=0;user-id=117166826;user-type=mod :testaccount_420!testaccount_420@testaccount_420.tmi.twitch.tv PRIVMSG #pajlada :@asd";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();
        assert_eq!(msg.sender.name, "테스트계정420");
    }

    #[test]
    fn test_display_name_with_middle_space() {
        let src = "@badge-info=;badges=;color=;display-name=Riot\\sGames;emotes=;flags=;id=bdfa278e-11c4-484f-9491-0a61b16fab60;mod=1;room-id=36029255;subscriber=0;tmi-sent-ts=1593953876927;turbo=0;user-id=36029255;user-type= :riotgames!riotgames@riotgames.tmi.twitch.tv PRIVMSG #riotgames :test fake message";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();
        assert_eq!(msg.sender.name, "Riot Games");
        assert_eq!(msg.sender.login, "riotgames");
    }

    #[test]
    fn test_emotes_1() {
        let src = "@badge-info=subscriber/22;badges=moderator/1,subscriber/12;color=#19E6E6;display-name=randers;emotes=1902:6-10,29-33,35-39/499:45-46,48-49/490:51-52/25:0-4,12-16,18-22;flags=;id=f9c5774b-faa7-4378-b1af-c4e08b532dc2;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1594556065407;turbo=0;user-id=40286300;user-type=mod :randers!randers@randers.tmi.twitch.tv PRIVMSG #pajlada :Kappa Keepo Kappa Kappa test Keepo Keepo 123 :) :) :P";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();
        assert_eq!(
            msg.emotes,
            vec![
                Emote {
                    id: "25".to_owned(),
                    char_range: Range { start: 0, end: 5 },
                    code: "Kappa".to_owned()
                },
                Emote {
                    id: "1902".to_owned(),
                    char_range: Range { start: 6, end: 11 },
                    code: "Keepo".to_owned()
                },
                Emote {
                    id: "25".to_owned(),
                    char_range: Range { start: 12, end: 17 },
                    code: "Kappa".to_owned()
                },
                Emote {
                    id: "25".to_owned(),
                    char_range: Range { start: 18, end: 23 },
                    code: "Kappa".to_owned()
                },
                Emote {
                    id: "1902".to_owned(),
                    char_range: Range { start: 29, end: 34 },
                    code: "Keepo".to_owned()
                },
                Emote {
                    id: "1902".to_owned(),
                    char_range: Range { start: 35, end: 40 },
                    code: "Keepo".to_owned()
                },
                Emote {
                    id: "499".to_owned(),
                    char_range: Range { start: 45, end: 47 },
                    code: ":)".to_owned()
                },
                Emote {
                    id: "499".to_owned(),
                    char_range: Range { start: 48, end: 50 },
                    code: ":)".to_owned()
                },
                Emote {
                    id: "490".to_owned(),
                    char_range: Range { start: 51, end: 53 },
                    code: ":P".to_owned()
                },
            ]
        );
    }

    #[test]
    fn test_emote_non_numeric_id() {
        // emote tag specifies an index that's out of bounds.
        let src = "@badge-info=;badges=;client-nonce=245b864d508a69a685e25104204bd31b;color=#FF144A;display-name=AvianArtworks;emote-only=1;emotes=300196486_TK:0-7;flags=;id=21194e0d-f0fa-4a8f-a14f-3cbe89366ad9;mod=0;room-id=11148817;subscriber=0;tmi-sent-ts=1594552113129;turbo=0;user-id=39565465;user-type= :avianartworks!avianartworks@avianartworks.tmi.twitch.tv PRIVMSG #pajlada :pajaM_TK";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();
        assert_eq!(
            msg.emotes,
            vec![Emote {
                id: "300196486_TK".to_owned(),
                char_range: Range { start: 0, end: 8 },
                code: "pajaM_TK".to_owned()
            },]
        );
    }

    #[test]
    fn test_emote_after_emoji() {
        // emojis are wider than one byte, tests that indices correctly refer
        // to unicode scalar values, and not bytes in the utf-8 FastStr
        let src = "@badge-info=subscriber/22;badges=moderator/1,subscriber/12;color=#19E6E6;display-name=randers;emotes=483:2-3,7-8,12-13;flags=;id=3695cb46-f70a-4d6f-a71b-159d434c45b5;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1594557379272;turbo=0;user-id=40286300;user-type=mod :randers!randers@randers.tmi.twitch.tv PRIVMSG #pajlada :👉 <3 👉 <3 👉 <3";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();
        assert_eq!(
            msg.emotes,
            vec![
                Emote {
                    id: "483".to_owned(),
                    char_range: Range { start: 2, end: 4 },
                    code: "<3".to_owned()
                },
                Emote {
                    id: "483".to_owned(),
                    char_range: Range { start: 7, end: 9 },
                    code: "<3".to_owned()
                },
                Emote {
                    id: "483".to_owned(),
                    char_range: Range { start: 12, end: 14 },
                    code: "<3".to_owned()
                },
            ]
        );
    }

    #[test]
    fn test_message_with_bits() {
        let src = "@badge-info=;badges=bits/100;bits=1;color=#004B49;display-name=TETYYS;emotes=;flags=;id=d7f03a35-f339-41ca-b4d4-7c0721438570;mod=0;room-id=11148817;subscriber=0;tmi-sent-ts=1594571566672;turbo=0;user-id=36175310;user-type= :tetyys!tetyys@tetyys.tmi.twitch.tv PRIVMSG #pajlada :trihard1";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();
        assert_eq!(msg.bits, Some(1));
    }

    #[test]
    fn test_incorrect_emote_index() {
        // emote index off by one.
        let src = r"@badge-info=;badges=;color=;display-name=some_1_happy;emotes=425618:49-51;flags=24-28:A.3;id=9eb37414-0952-44cc-b177-ad8007088034;mod=0;room-id=35768443;subscriber=0;tmi-sent-ts=1597921035256;turbo=0;user-id=473035780;user-type= :some_1_happy!some_1_happy@some_1_happy.tmi.twitch.tv PRIVMSG #mocbka34 :Я не такой красивый. Не урод, но до тебя далеко LUL";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.emotes,
            vec![Emote {
                id: "425618".to_owned(),
                char_range: 49..52,
                code: "UL".to_owned(),
            }]
        );
        assert_eq!(
            msg.message_text,
            "Я не такой красивый. Не урод, но до тебя далеко LUL"
        );
    }

    #[test]
    fn test_extremely_incorrect_emote_index() {
        // emote index off by more than 1
        let src = r"@badge-info=subscriber/3;badges=subscriber/3;color=#0000FF;display-name=Linkoping;emotes=25:41-45;flags=17-26:S.6;id=744f9c58-b180-4f46-bd9e-b515b5ef75c1;mod=0;room-id=188442366;subscriber=1;tmi-sent-ts=1566335866017;turbo=0;user-id=91673457;user-type= :linkoping!linkoping@linkoping.tmi.twitch.tv PRIVMSG #queenqarro :Då kan du begära skadestånd och förtal Kappa";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.emotes,
            vec![Emote {
                id: "25".to_owned(),
                char_range: 41..46,
                code: "ppa".to_owned(),
            }]
        );
        assert_eq!(
            msg.message_text,
            "Då kan du begära skadestånd och förtal Kappa"
        );
    }

    #[test]
    fn test_emote_index_complete_out_of_range() {
        // no overlap between FastStr and specified range
        let src = r"@badge-info=subscriber/3;badges=subscriber/3;color=#0000FF;display-name=Linkoping;emotes=25:44-48;flags=17-26:S.6;id=744f9c58-b180-4f46-bd9e-b515b5ef75c1;mod=0;room-id=188442366;subscriber=1;tmi-sent-ts=1566335866017;turbo=0;user-id=91673457;user-type= :linkoping!linkoping@linkoping.tmi.twitch.tv PRIVMSG #queenqarro :Då kan du begära skadestånd och förtal Kappa";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.emotes,
            vec![Emote {
                id: "25".to_owned(),
                char_range: 44..49,
                code: "".to_owned(),
            }]
        );
    }

    #[test]
    fn test_emote_index_beyond_out_of_range() {
        // no overlap between FastStr and specified range
        let src = r"@badge-info=subscriber/3;badges=subscriber/3;color=#0000FF;display-name=Linkoping;emotes=25:45-49;flags=17-26:S.6;id=744f9c58-b180-4f46-bd9e-b515b5ef75c1;mod=0;room-id=188442366;subscriber=1;tmi-sent-ts=1566335866017;turbo=0;user-id=91673457;user-type= :linkoping!linkoping@linkoping.tmi.twitch.tv PRIVMSG #queenqarro :Då kan du begära skadestånd och förtal Kappa";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.emotes,
            vec![Emote {
                id: "25".to_owned(),
                char_range: 45..50,
                code: "".to_owned(),
            }]
        );
    }
}
