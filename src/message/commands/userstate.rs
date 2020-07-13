use crate::message::commands::IRCMessageParseExt;
use crate::message::twitch::{Badge, RGBColor};
use crate::message::{IRCMessage, ServerMessageParseError};
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct UserStateMessage {
    // same content as GlobalUserStateMessage, but in the context of a channel_login and missing user_id.
    pub channel_login: String,
    pub user_name: String,
    pub badge_info: Vec<Badge>,
    pub badges: Vec<Badge>,
    pub emote_sets: Vec<String>,
    pub name_color: Option<RGBColor>,

    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for UserStateMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<UserStateMessage, ServerMessageParseError> {
        if source.command != "USERSTATE" {
            return Err(ServerMessageParseError::MismatchedCommand());
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
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = UserStateMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            UserStateMessage {
                channel_login: "randers".to_owned(),
                user_name: "TESTUSER".to_owned(),
                badge_info: vec![],
                badges: vec![],
                emote_sets: vec!["0".to_owned()],
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
