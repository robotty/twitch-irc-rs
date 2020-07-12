use crate::message::commands::IRCMessageParseExt;
use crate::message::twitch::{Badge, Emote, RGBColor, TwitchUserBasics};
use crate::message::{IRCMessage, ServerMessageParseError};
use chrono::{DateTime, Utc};
use derivative::Derivative;
use std::convert::TryFrom;

#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PrivmsgMessage {
    pub channel_login: String,
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
    source: IRCMessage,
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
            sender: TwitchUserBasics {
                id: source.try_get_nonempty_tag_value("user-id")?,
                login: source.try_get_prefix_nickname()?,
                name: source.try_get_nonempty_tag_value("display-name")?,
            },
            badge_info: source.try_get_badges("badge-info")?,
            badges: source.try_get_badges("badges")?,
            bits: source.try_get_bits("bits")?,
            name_color: source.try_get_color("color")?,
            emotes: source.try_get_emotes("emotes", &message_text)?,
            server_timestamp: source.try_get_timestamp("tmi-sent-ts")?,
            message_id: source.try_get_nonempty_tag_value("id")?,
            message_text,
            action,
            source,
        })
    }
}

impl From<PrivmsgMessage> for IRCMessage {
    fn from(msg: PrivmsgMessage) -> IRCMessage {
        // TODO make it so you can construct a PrivmsgMessage from all the parameters
        //  too, and then synthesize it into a IRCMessage
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::twitch::{RGBColor, TwitchUserBasics};
    use crate::message::{IRCMessage, PrivmsgMessage};
    use chrono::offset::TimeZone;
    use chrono::Utc;
    use std::convert::TryFrom;

    // ACTION
    // badges, badge-info
    // color emptystring (greynames)
    // display name with trailing space
    // display name with hieroglyphs
    // emotes
    // emotes that are out of bounds
    //

    #[test]
    fn test_basic_example() {
        let src = "@badge-info=;badges=;color=#0000FF;display-name=JuN1oRRRR;emotes=;flags=;id=e9d998c3-36f1-430f-89ec-6b887c28af36;mod=0;room-id=11148817;subscriber=0;tmi-sent-ts=1594545155039;turbo=0;user-id=29803735;user-type= :jun1orrrr!jun1orrrr@jun1orrrr.tmi.twitch.tv PRIVMSG #pajlada :dank cam";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PrivmsgMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PrivmsgMessage {
                channel_login: "pajlada".to_owned(),
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
}
