use crate::message::commands::IRCMessageParseExt;
use crate::message::{IRCMessage, ServerMessageParseError};
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct NoticeMessage {
    pub channel_login: Option<String>,
    pub message_text: String,
    pub message_id: Option<String>,

    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for NoticeMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<NoticeMessage, ServerMessageParseError> {
        if source.command != "NOTICE" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(NoticeMessage {
            channel_login: source
                .try_get_optional_channel_login()?
                .map(|s| s.to_owned()),
            message_text: source.try_get_param(1)?.to_owned(),
            message_id: source
                .try_get_optional_nonempty_tag_value("msg-id")?
                .map(|s| s.to_owned()),
            source,
        })
    }
}

impl From<NoticeMessage> for IRCMessage {
    fn from(msg: NoticeMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::{IRCMessage, NoticeMessage};
    use std::convert::TryFrom;

    #[test]
    pub fn test_basic() {
        let src = "@msg-id=msg_banned :tmi.twitch.tv NOTICE #forsen :You are permanently banned from talking in forsen.";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = NoticeMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            NoticeMessage {
                channel_login: Some("forsen".to_owned()),
                message_text: "You are permanently banned from talking in forsen.".to_owned(),
                message_id: Some("msg_banned".to_owned()),
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_pre_login() {
        // this style of notice is received before successful login
        let src = ":tmi.twitch.tv NOTICE * :Improperly formatted auth";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = NoticeMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            NoticeMessage {
                channel_login: None,
                message_text: "Improperly formatted auth".to_owned(),
                message_id: None,
                source: irc_message
            }
        )
    }
}
