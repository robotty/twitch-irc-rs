use crate::message::commands::IRCMessageParseExt;
use crate::message::twitch::{Badge, Emote, RGBColor, TwitchUserBasics};
use crate::message::{IRCMessage, ServerMessageParseError};
use chrono::{DateTime, Utc};
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PrivmsgMessage {
    pub channel_login: String,
    pub channel_id: String,
    pub message_text: String,
    pub action: bool,
    pub sender: TwitchUserBasics,
    pub badge_info: Vec<Badge>,
    pub badges: Vec<Badge>,
    pub bits: Option<u64>,
    pub name_color: Option<RGBColor>,
    pub emotes: Vec<Emote>,
    pub server_timestamp: DateTime<Utc>,
    pub message_id: String,

    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for PrivmsgMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PrivmsgMessage, ServerMessageParseError> {
        if source.command != "PRIVMSG" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        let mut message_text = source.try_get_param(1)?;
        let mut action = false;
        if message_text.starts_with("\u{0001}ACTION ") && message_text.ends_with('\u{0001}') {
            message_text = message_text[8..message_text.len() - 1].to_owned();
            action = true;
        }

        Ok(PrivmsgMessage {
            channel_login: source.try_get_channel_login()?,
            channel_id: source.try_get_nonempty_tag_value("room-id")?.to_owned(),
            sender: TwitchUserBasics {
                id: source.try_get_nonempty_tag_value("user-id")?.to_owned(),
                login: source.try_get_prefix_nickname()?,
                name: source
                    .try_get_nonempty_tag_value("display-name")?
                    .to_owned(),
            },
            badge_info: source.try_get_badges("badge-info")?,
            badges: source.try_get_badges("badges")?,
            bits: source.try_get_bits("bits")?,
            name_color: source.try_get_color("color")?,
            emotes: source.try_get_emotes("emotes", &message_text)?,
            server_timestamp: source.try_get_timestamp("tmi-sent-ts")?,
            message_id: source.try_get_nonempty_tag_value("id")?.to_owned(),
            message_text,
            action,
            source,
        })
    }
}

impl From<PrivmsgMessage> for IRCMessage {
    fn from(msg: PrivmsgMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::twitch::{Badge, Emote, RGBColor, TwitchUserBasics};
    use crate::message::{IRCMessage, PrivmsgMessage, ServerMessageParseError};
    use chrono::offset::TimeZone;
    use chrono::Utc;
    use std::convert::TryFrom;
    use std::ops::Range;

    #[test]
    fn test_basic_example() {
        let src = "@badge-info=;badges=;color=#0000FF;display-name=JuN1oRRRR;emotes=;flags=;id=e9d998c3-36f1-430f-89ec-6b887c28af36;mod=0;room-id=11148817;subscriber=0;tmi-sent-ts=1594545155039;turbo=0;user-id=29803735;user-type= :jun1orrrr!jun1orrrr@jun1orrrr.tmi.twitch.tv PRIVMSG #pajlada :dank cam";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PrivmsgMessage {
                channel_login: "pajlada".to_owned(),
                channel_id: "11148817".to_owned(),
                message_text: "dank cam".to_owned(),
                action: false,
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
                server_timestamp: Utc.timestamp_millis(1594545155039),
                message_id: "e9d998c3-36f1-430f-89ec-6b887c28af36".to_owned(),

                source: irc_message
            }
        );
    }

    #[test]
    fn test_action_and_badges() {
        let src = "@badge-info=subscriber/22;badges=moderator/1,subscriber/12;color=#19E6E6;display-name=randers;emotes=;flags=;id=d831d848-b7c7-4559-ae3a-2cb88f4dbfed;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1594555275886;turbo=0;user-id=40286300;user-type=mod :randers!randers@randers.tmi.twitch.tv PRIVMSG #pajlada :ACTION -tags";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PrivmsgMessage {
                channel_login: "pajlada".to_owned(),
                channel_id: "11148817".to_owned(),
                message_text: "-tags".to_owned(),
                action: true,
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
                server_timestamp: Utc.timestamp_millis(1594555275886),
                message_id: "d831d848-b7c7-4559-ae3a-2cb88f4dbfed".to_owned(),

                source: irc_message
            }
        );
    }

    #[test]
    fn test_greyname_no_color() {
        let src = "@rm-received-ts=1594554085918;historical=1;badge-info=;badges=;client-nonce=815810609edecdf4537bd9586994182b;color=;display-name=CarvedTaleare;emotes=;flags=;id=c9b941d9-a0ab-4534-9903-971768fcdf10;mod=0;room-id=22484632;subscriber=0;tmi-sent-ts=1594554085753;turbo=0;user-id=467684514;user-type= :carvedtaleare!carvedtaleare@carvedtaleare.tmi.twitch.tv PRIVMSG #forsen :NaM";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PrivmsgMessage {
                channel_login: "forsen".to_owned(),
                channel_id: "22484632".to_owned(),
                message_text: "NaM".to_owned(),
                action: false,
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
                server_timestamp: Utc.timestamp_millis(1594554085753),
                message_id: "c9b941d9-a0ab-4534-9903-971768fcdf10".to_owned(),

                source: irc_message
            }
        );
    }

    #[test]
    fn test_display_name_with_trailing_space() {
        let src = "@rm-received-ts=1594554085918;historical=1;badge-info=;badges=;client-nonce=815810609edecdf4537bd9586994182b;color=;display-name=CarvedTaleare\\s;emotes=;flags=;id=c9b941d9-a0ab-4534-9903-971768fcdf10;mod=0;room-id=22484632;subscriber=0;tmi-sent-ts=1594554085753;turbo=0;user-id=467684514;user-type= :carvedtaleare!carvedtaleare@carvedtaleare.tmi.twitch.tv PRIVMSG #forsen :NaM";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();
        assert_eq!(msg.sender.name, "CarvedTaleare ");
    }

    #[test]
    fn test_korean_display_name() {
        let src = "@badge-info=subscriber/35;badges=moderator/1,subscriber/3024;color=#FF0000;display-name=테스트계정420;emotes=;flags=;id=bdfa278e-11c4-484f-9491-0a61b16fab60;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1593953876927;turbo=0;user-id=117166826;user-type=mod :testaccount_420!testaccount_420@testaccount_420.tmi.twitch.tv PRIVMSG #pajlada :@asd";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();
        assert_eq!(msg.sender.name, "테스트계정420");
    }

    #[test]
    fn test_display_name_with_middle_space() {
        let src = "@badge-info=;badges=;color=;display-name=Riot\\sGames;emotes=;flags=;id=bdfa278e-11c4-484f-9491-0a61b16fab60;mod=1;room-id=36029255;subscriber=0;tmi-sent-ts=1593953876927;turbo=0;user-id=36029255;user-type= :riotgames!riotgames@riotgames.tmi.twitch.tv PRIVMSG #riotgames :test fake message";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();
        assert_eq!(msg.sender.name, "Riot Games");
        assert_eq!(msg.sender.login, "riotgames");
    }

    #[test]
    fn test_emotes_1() {
        let src = "@badge-info=subscriber/22;badges=moderator/1,subscriber/12;color=#19E6E6;display-name=randers;emotes=1902:6-10,29-33,35-39/499:45-46,48-49/490:51-52/25:0-4,12-16,18-22;flags=;id=f9c5774b-faa7-4378-b1af-c4e08b532dc2;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1594556065407;turbo=0;user-id=40286300;user-type=mod :randers!randers@randers.tmi.twitch.tv PRIVMSG #pajlada :Kappa Keepo Kappa Kappa test Keepo Keepo 123 :) :) :P";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();
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
    fn test_emote_index_out_of_bounds() {
        // emote tag specifies an index that's out of bounds.
        let src = "@badge-info=subscriber/3;badges=subscriber/3;color=#0000FF;display-name=Linkoping;emotes=25:40-44;flags=17-26:S.6;id=744f9c58-b180-4f46-bd9e-b515b5ef75c1;mod=0;room-id=188442366;subscriber=1;tmi-sent-ts=1566335866017;turbo=0;user-id=91673457;user-type= :linkoping!linkoping@linkoping.tmi.twitch.tv PRIVMSG #queenqarro :Då kan du begära skadestånd och förtal Kappa";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let result = PrivmsgMessage::try_from(irc_message.clone());
        assert_eq!(
            result.unwrap_err(),
            ServerMessageParseError::MalformedTagValue("emotes", "25:40-44".to_owned())
        );
    }

    #[test]
    fn test_emote_non_numeric_id() {
        // emote tag specifies an index that's out of bounds.
        let src = "@badge-info=;badges=;client-nonce=245b864d508a69a685e25104204bd31b;color=#FF144A;display-name=AvianArtworks;emote-only=1;emotes=300196486_TK:0-7;flags=;id=21194e0d-f0fa-4a8f-a14f-3cbe89366ad9;mod=0;room-id=11148817;subscriber=0;tmi-sent-ts=1594552113129;turbo=0;user-id=39565465;user-type= :avianartworks!avianartworks@avianartworks.tmi.twitch.tv PRIVMSG #pajlada :pajaM_TK";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();
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
        // to unicode scalar values, and not bytes in the utf-8 string
        let src = "@badge-info=subscriber/22;badges=moderator/1,subscriber/12;color=#19E6E6;display-name=randers;emotes=483:2-3,7-8,12-13;flags=;id=3695cb46-f70a-4d6f-a71b-159d434c45b5;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1594557379272;turbo=0;user-id=40286300;user-type=mod :randers!randers@randers.tmi.twitch.tv PRIVMSG #pajlada :👉 <3 👉 <3 👉 <3";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();
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
}
