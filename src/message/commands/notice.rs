use fast_str::FastStr;

use crate::message::commands::IRCMessageParseExt;
use crate::message::{IRCMessage, ServerMessageParseError};

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

/// A user-facing notice sent by the server.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "with-serde",
    derive(
        Serialize,
        Deserialize
    )
)]
pub struct NoticeMessage {
    /// The login name of the channel that this notice was sent to. There are cases where this
    /// is missing, for example when a `NOTICE` message is sent in response to a failed login
    /// attempt.
    pub channel_login: Option<FastStr>,
    /// Message content of the notice. This is some user-friendly FastStr, e.g.
    /// `You are permanently banned from talking in <channel>.`
    pub message_text: FastStr,
    /// If present, a computer-readable FastStr identifying the class/type of notice.
    /// For example `msg_banned`. These message IDs are [documented by Twitch here](https://dev.twitch.tv/docs/irc/msg-id).
    pub message_id: Option<FastStr>,

    /// The message that this `NoticeMessage` was parsed from.
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for NoticeMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<NoticeMessage, ServerMessageParseError> {
        if source.command != "NOTICE" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
        }
        Ok(NoticeMessage {
            channel_login: {
                match source.try_get_optional_channel_login()? {
                    Some(channel_login) => Some(FastStr::from_ref(channel_login)),
                    None => None,
                }
            },
            message_text: FastStr::from_ref(source.try_get_param(1)?),
            message_id: {
                match source.try_get_optional_nonempty_tag_value("msg-id")? {
                    Some(message_id) => Some(FastStr::from_ref(message_id)),
                    None => None,
                }
            },
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
                channel_login: Some("forsen".into()),
                message_text: "You are permanently banned from talking in forsen.".into(),
                message_id: Some("msg_banned".into()),
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
                message_text: "Improperly formatted auth".into(),
                message_id: None,
                source: irc_message
            }
        )
    }
}
