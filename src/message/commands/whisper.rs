use crate::message::commands::IRCMessageParseExt;
use crate::message::twitch::{Badge, Emote, RGBColor, TwitchUserBasics};
use crate::message::{IRCMessage, ServerMessageParseError};
use std::convert::TryFrom;

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};
/// A incoming whisper message (a private user-to-user message).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct WhisperMessage {
    /// The login name of the receiving user (the logged in user).
    pub recipient_login: String,
    /// User details of the user that sent us this whisper (the sending user).
    pub sender: TwitchUserBasics,
    /// The text content of the message.
    pub message_text: String,
    /// Name color of the sending user.
    pub name_color: Option<RGBColor>,
    /// List of badges (that the sending user has) that should be displayed alongside the message.
    pub badges: Vec<Badge>,
    /// A list of emotes in this message. Each emote replaces a part of the `message_text`.
    /// These emotes are sorted in the order that they appear in the message.
    pub emotes: Vec<Emote>,

    /// The message that this `WhisperMessage` was parsed from.
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for WhisperMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<WhisperMessage, ServerMessageParseError> {
        if source.command != "WHISPER" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
        }

        // example:
        // @badges=;color=#19E6E6;display-name=randers;emotes=25:22-26;message-id=1;thread-id=40286300_553170741;turbo=0;user-id=40286300;user-type= :randers!randers@randers.tmi.twitch.tv WHISPER randers811 :hello, this is a test Kappa

        let message_text = source.try_get_param(1)?.to_owned();
        let emotes = source.try_get_emotes("emotes", &message_text)?;

        Ok(WhisperMessage {
            recipient_login: source.try_get_param(0)?.to_owned(),
            sender: TwitchUserBasics {
                id: source.try_get_nonempty_tag_value("user-id")?.to_owned(),
                login: source.try_get_prefix_nickname()?.to_owned(),
                name: source
                    .try_get_nonempty_tag_value("display-name")?
                    .to_owned(),
            },
            message_text,
            name_color: source.try_get_color("color")?,
            badges: source.try_get_badges("badges")?,
            emotes,
            source,
        })
    }
}

impl From<WhisperMessage> for IRCMessage {
    fn from(msg: WhisperMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::twitch::{Emote, RGBColor, TwitchUserBasics};
    use crate::message::{IRCMessage, WhisperMessage};
    use std::convert::TryFrom;
    use std::ops::Range;

    #[test]
    pub fn test_basic() {
        let src = "@badges=;color=#19E6E6;display-name=randers;emotes=25:22-26;message-id=1;thread-id=40286300_553170741;turbo=0;user-id=40286300;user-type= :randers!randers@randers.tmi.twitch.tv WHISPER randers811 :hello, this is a test Kappa";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = WhisperMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            WhisperMessage {
                recipient_login: "randers811".to_owned(),
                sender: TwitchUserBasics {
                    id: "40286300".to_owned(),
                    login: "randers".to_owned(),
                    name: "randers".to_owned()
                },
                message_text: "hello, this is a test Kappa".to_owned(),
                name_color: Some(RGBColor {
                    r: 0x19,
                    g: 0xE6,
                    b: 0xE6
                }),
                badges: vec![],
                emotes: vec![Emote {
                    id: "25".to_owned(),
                    char_range: Range { start: 22, end: 27 },
                    code: "Kappa".to_owned()
                }],
                source: irc_message
            },
        )
    }

    // note, I have tested and there is no support for \u0001ACTION <message>\u0001 style actions
    // via whispers. (the control character gets filtered.) - so there is no special case to test
}
