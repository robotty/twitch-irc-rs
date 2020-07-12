use crate::message::commands::{IRCMessageParseExt, ServerMessageParseError};
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PartMessage {
    pub channel_login: String,
    pub user_login: String,
    #[derivative(PartialEq = "ignore")]
    source: IRCMessage,
}

impl TryFrom<IRCMessage> for PartMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PartMessage, ServerMessageParseError> {
        if source.command != "PART" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(PartMessage {
            channel_login: source.try_get_channel_login()?,
            user_login: source.try_get_prefix_nickname()?,
            source,
        })
    }
}

impl From<PartMessage> for IRCMessage {
    fn from(msg: PartMessage) -> IRCMessage {
        msg.source
    }
}
