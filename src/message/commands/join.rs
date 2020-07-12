use crate::message::commands::{IRCMessageParseExt, ServerMessageParseError};
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct JoinMessage {
    pub channel_login: String,
    pub user_login: String,
    #[derivative(PartialEq = "ignore")]
    source: IRCMessage,
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
            source,
        })
    }
}

impl From<JoinMessage> for IRCMessage {
    fn from(msg: JoinMessage) -> IRCMessage {
        msg.source
    }
}
