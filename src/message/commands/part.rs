use crate::message::commands::{AsIRCMessage, IRCMessageParseExt, ServerMessageParseError};
use crate::message::prefix::IRCPrefix;
use crate::message::tags::IRCTags;
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PartMessage {
    pub channel_login: String,
    pub user_login: String,
    #[derivative(PartialEq = "ignore")]
    source: Option<IRCMessage>,
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
            source: Some(source),
        })
    }
}

impl AsIRCMessage for PartMessage {
    fn as_irc_message(&self) -> IRCMessage {
        if let Some(source) = &self.source {
            source.clone()
        } else {
            // :user_login JOIN #channel_login
            IRCMessage::new(
                IRCTags::new(),
                Some(IRCPrefix::Full {
                    nick: self.user_login.clone(),
                    user: None,
                    host: None,
                }),
                "PART".to_owned(),
                vec![format!("#{}", self.channel_login)],
            )
        }
    }
}
