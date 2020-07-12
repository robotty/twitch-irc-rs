use crate::message::commands::IRCMessageParseExt;
use crate::message::twitch::{Badge, Emote, RGBColor, TwitchUserBasics};
use crate::message::{IRCMessage, ServerMessageParseError};
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
    pub name_color: RGBColor,
    pub emotes: Vec<Emote>,

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
        if message_text.starts_with("\u{0001}ACTION ") && message_text.ends_with("\u{0001}") {
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
