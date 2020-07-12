use crate::message::commands::{IRCMessageParseExt, ServerMessageParseError};
use crate::message::prefix::IRCPrefix;
use crate::message::tags::IRCTags;
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct JoinMessage {
    pub channel_login: String,
    pub user_login: String,
    #[derivative(PartialEq = "ignore")]
    source: Option<IRCMessage>,
}

impl TryFrom<IRCMessage> for JoinMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<JoinMessage, ServerMessageParseError> {
        if source.command != "JOIN" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(JoinMessage {
            channel_login: source.try_get_channel_login()?,
            user_login: source.try_get_prefix_nickname()?,
            source: Some(source),
        })
    }
}

impl From<JoinMessage> for IRCMessage {
    fn from(msg: JoinMessage) -> IRCMessage {
        // FIXME this breaks when you mutate the parsed message and then try to convert it back.
        //  same issue on other message types.
        if let Some(source) = msg.source {
            source
        } else {
            // :user_login JOIN #channel_login
            IRCMessage::new(
                IRCTags::new(),
                Some(IRCPrefix::Full {
                    nick: msg.user_login,
                    user: None,
                    host: None,
                }),
                "JOIN".to_owned(),
                vec![format!("#{}", msg.channel_login)],
            )
        }
    }
}
