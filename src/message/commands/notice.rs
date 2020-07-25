use crate::message::commands::IRCMessageParseExt;
use crate::message::{IRCMessage, ServerMessageParseError};
use std::convert::TryFrom;

/// A user-facing notice sent by the server.
#[derive(Debug, Clone, PartialEq)]
pub struct NoticeMessage {
    /// The login name of the channel that this notice was sent to. There are cases where this
    /// is missing, for example when a `NOTICE` message is sent in response to a failed login
    /// attempt.
    pub channel_login: Option<String>,
    /// Message content of the notice. This is some user-friendly string, e.g.
    /// `You are permanently banned from talking in <channel>.`
    pub message_text: String,
    /// If present, a computer-readable string identifying the class/type of notice.
    /// For example `msg_banned`. These message IDs are [documented by Twitch here](https://dev.twitch.tv/docs/irc/msg-id).
    pub message_id: Option<String>,

    /// The message that this `NoticeMessage` was parsed from.
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
