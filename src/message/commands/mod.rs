pub mod join;
pub mod part;
pub mod ping;
pub mod pong;
pub mod reconnect;

use self::ServerMessageParseError::*;
use crate::message::commands::join::JoinMessage;
use crate::message::commands::part::PartMessage;
use crate::message::commands::ping::PingMessage;
use crate::message::commands::pong::PongMessage;
use crate::message::commands::reconnect::ReconnectMessage;
use crate::message::prefix::IRCPrefix;
use crate::message::{AsRawIRC, IRCMessage};
use std::convert::TryFrom;
use std::fmt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServerMessageParseError {
    #[error("That command's data is not parsed by this implementation")]
    MismatchedCommand(),
    #[error("No tag present under key {0}")]
    MissingTag(&'static str),
    #[error("No tag value present under key {0}")]
    MissingTagValue(&'static str),
    #[error("No parameter found at index {0}")]
    MissingParameter(usize),
    #[error("Malformed channel parameter (# must be present + something after it)")]
    MalformedChannel(),
    #[error("Missing prefix altogether")]
    MissingPrefix(),
    #[error("No nickname found in prefix")]
    MissingNickname(),
}

trait IRCMessageParseExt {
    fn try_get_param(&self, index: usize) -> Result<String, ServerMessageParseError>;
    fn try_get_tag_value(
        &self,
        key: &'static str,
    ) -> Result<Option<String>, ServerMessageParseError>;
    fn try_get_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<String, ServerMessageParseError>;
    fn try_get_channel_login(&self) -> Result<String, ServerMessageParseError>;
    fn try_get_prefix_nickname(&self) -> Result<String, ServerMessageParseError>;
}

impl IRCMessageParseExt for IRCMessage {
    fn try_get_param(&self, index: usize) -> Result<String, ServerMessageParseError> {
        Ok(self
            .params
            .get(index)
            .ok_or(MissingParameter(index))?
            .clone())
    }

    fn try_get_tag_value(
        &self,
        key: &'static str,
    ) -> Result<Option<String>, ServerMessageParseError> {
        Ok(self.tags.0.get(key).ok_or(MissingTag(key))?.clone())
    }

    fn try_get_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<String, ServerMessageParseError> {
        Ok(self.try_get_tag_value(key)?.ok_or(MissingTagValue(key))?)
    }

    fn try_get_channel_login(&self) -> Result<String, ServerMessageParseError> {
        let param = self.try_get_param(0)?;

        if !param.starts_with('#') || param.len() < 2 {
            return Err(MalformedChannel());
        }

        Ok(String::from(&param[1..]))
    }

    fn try_get_prefix_nickname(&self) -> Result<String, ServerMessageParseError> {
        match &self.prefix {
            None => Err(MissingPrefix()),
            Some(IRCPrefix::HostOnly { host: _ }) => Err(MissingNickname()),
            Some(IRCPrefix::Full {
                nick,
                user: _,
                host: _,
            }) => Ok(nick.clone()),
        }
    }
}

pub trait AsIRCMessage {
    fn as_irc_message(&self) -> IRCMessage;
}

impl<T> AsRawIRC for T
where
    T: AsIRCMessage,
{
    fn format_as_raw_irc(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_irc_message().format_as_raw_irc(f)
    }
}

// makes it so users cannot match against Generic and get the underlying IRCMessage
// that way (which would break their implementations if there is an enum variant added and they
// expect certain commands to be emitted under Generic)
// that means the only way to get the IRCMessage is via as_irc_message()
// which combined with #[non_exhaustive] allows us to add enum variants
// without making a major release
#[derive(Debug, PartialEq, Clone)]
#[doc(hidden)]
pub struct HiddenIRCMessage(pub(self) IRCMessage);

#[derive(Debug, PartialEq, Clone)]
#[non_exhaustive]
pub enum ServerMessage {
    Join(JoinMessage),
    Part(PartMessage),
    Ping(PingMessage),
    Pong(PongMessage),
    Reconnect(ReconnectMessage),
    #[doc(hidden)]
    Generic(HiddenIRCMessage),
}

impl TryFrom<IRCMessage> for ServerMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<ServerMessage, ServerMessageParseError> {
        use ServerMessage::*;

        Ok(match source.command.as_str() {
            "JOIN" => Join(JoinMessage::try_from(source)?),
            "PART" => Part(PartMessage::try_from(source)?),
            "PING" => Ping(PingMessage::try_from(source)?),
            "PONG" => Pong(PongMessage::try_from(source)?),
            "RECONNECT" => Reconnect(ReconnectMessage::try_from(source)?),
            _ => Generic(HiddenIRCMessage(source)),
        })
    }
}

impl AsIRCMessage for ServerMessage {
    fn as_irc_message(&self) -> IRCMessage {
        match self {
            ServerMessage::Join(msg) => msg.as_irc_message(),
            ServerMessage::Part(msg) => msg.as_irc_message(),
            ServerMessage::Ping(msg) => msg.as_irc_message(),
            ServerMessage::Pong(msg) => msg.as_irc_message(),
            ServerMessage::Reconnect(msg) => msg.as_irc_message(),
            ServerMessage::Generic(msg) => msg.0.clone(),
        }
    }
}
