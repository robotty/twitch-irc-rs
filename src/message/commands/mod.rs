pub mod join;
pub mod part;
pub mod ping;
pub mod pong;
pub mod privmsg;
pub mod reconnect;

use self::ServerMessageParseError::*;
use crate::message::commands::join::JoinMessage;
use crate::message::commands::part::PartMessage;
use crate::message::commands::ping::PingMessage;
use crate::message::commands::pong::PongMessage;
use crate::message::commands::reconnect::ReconnectMessage;
use crate::message::prefix::IRCPrefix;
use crate::message::twitch::{Badge, Emote, RGBColor};
use crate::message::{IRCMessage, PrivmsgMessage};
use chrono::{DateTime, TimeZone, Utc};
use itertools::Itertools;
use std::convert::TryFrom;
use std::ops::Range;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ServerMessageParseError {
    #[error("That command's data is not parsed by this implementation")]
    MismatchedCommand(),
    #[error("No tag present under key {0}")]
    MissingTag(&'static str),
    #[error("No tag value present under key {0}")]
    MissingTagValue(&'static str),
    #[error("Malformed tag value for tag `{0}`, value was `{1}`")]
    MalformedTagValue(&'static str, String),
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
    fn try_get_tag_value(&self, key: &'static str)
        -> Result<Option<&str>, ServerMessageParseError>;
    fn try_get_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<&str, ServerMessageParseError>;
    fn try_get_channel_login(&self) -> Result<String, ServerMessageParseError>;
    fn try_get_prefix_nickname(&self) -> Result<String, ServerMessageParseError>;
    fn try_get_emotes(
        &self,
        tag_key: &'static str,
        message_text: &str,
    ) -> Result<Vec<Emote>, ServerMessageParseError>;
    fn try_get_badges(&self, tag_key: &'static str) -> Result<Vec<Badge>, ServerMessageParseError>;
    fn try_get_color(
        &self,
        tag_key: &'static str,
    ) -> Result<Option<RGBColor>, ServerMessageParseError>;
    fn try_get_bits(&self, tag_key: &'static str) -> Result<Option<u64>, ServerMessageParseError>;
    fn try_get_timestamp(
        &self,
        tag_key: &'static str,
    ) -> Result<DateTime<Utc>, ServerMessageParseError>;
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
    ) -> Result<Option<&str>, ServerMessageParseError> {
        match self.tags.0.get(key) {
            Some(Some(value)) => Ok(Some(value)),
            Some(None) => Ok(None),
            None => return Err(MissingTag(key)),
        }
    }

    fn try_get_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<&str, ServerMessageParseError> {
        match self.tags.0.get(key) {
            Some(Some(value)) => Ok(value),
            Some(None) => return Err(MissingTagValue(key)),
            None => return Err(MissingTag(key)),
        }
    }

    fn try_get_channel_login(&self) -> Result<String, ServerMessageParseError> {
        let param = self.try_get_param(0)?;

        if !param.starts_with('#') || param.len() < 2 {
            return Err(MalformedChannel());
        }

        Ok(String::from(&param[1..]))
    }

    /// Get the sending user's login name from the IRC prefix.
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

    fn try_get_emotes(
        &self,
        tag_key: &'static str,
        message_text: &str,
    ) -> Result<Vec<Emote>, ServerMessageParseError> {
        let tag_value = self.try_get_nonempty_tag_value(tag_key)?;

        if tag_value == "" {
            return Ok(vec![]);
        }

        let mut emotes = Vec::new();

        let make_error = || MalformedTagValue(tag_key, tag_value.to_owned());

        // emotes tag format:
        // emote_id:from-to,from-to,from-to/emote_id:from-to,from-to/emote_id:from-to
        for src in tag_value.split('/') {
            let (emote_id, indices_src) = src.splitn(2, ':').next_tuple().ok_or_else(make_error)?;

            for range_src in indices_src.split(',') {
                let (start, end) = range_src
                    .splitn(2, '-')
                    .next_tuple()
                    .ok_or_else(make_error)?;

                let start = usize::from_str(start).map_err(|_| make_error())?;
                // twitch specifies the end index as inclusive, but in Rust (and most programming
                // languages for that matter) it's very common to specify end indices as exclusive,
                // so we add 1 here to make it exclusive.
                let end = usize::from_str(end).map_err(|_| make_error())? + 1;

                let code_length = end - start;

                let code = message_text
                    .chars()
                    .skip(start)
                    .take(code_length)
                    .collect::<String>();

                // range specified in the emotes tag was out of range for the message text string
                if code.chars().count() != code_length {
                    return Err(make_error());
                }

                emotes.push(Emote {
                    id: emote_id.to_owned(),
                    char_range: Range { start, end },
                    code,
                });
            }
        }

        Ok(emotes)
    }

    fn try_get_badges(&self, tag_key: &'static str) -> Result<Vec<Badge>, ServerMessageParseError> {
        // TODO same thing as above, could be optimized to not clone the tag value as well
        let tag_value = self.try_get_nonempty_tag_value(tag_key)?;

        if tag_value == "" {
            return Ok(vec![]);
        }

        let mut badges = Vec::new();

        let make_error = || MalformedTagValue(tag_key, tag_value.to_owned());

        // badges tag format:
        // admin/1,moderator/1,subscriber/12
        for src in tag_value.split(',') {
            let (name, version) = src
                .splitn(2, '/')
                .map(|s| s.to_owned())
                .next_tuple()
                .ok_or_else(make_error)?;

            badges.push(Badge { name, version })
        }

        Ok(badges)
    }

    fn try_get_color(
        &self,
        tag_key: &'static str,
    ) -> Result<Option<RGBColor>, ServerMessageParseError> {
        let tag_value = self.try_get_nonempty_tag_value(tag_key)?;
        let make_error = || MalformedTagValue(tag_key, tag_value.to_owned());

        // color is expected to be in format #RRGGBB
        if tag_value.len() != 7 {
            return Err(make_error());
        }

        Ok(Some(RGBColor {
            r: u8::from_str_radix(&tag_value[1..3], 16).map_err(|_| make_error())?,
            g: u8::from_str_radix(&tag_value[3..5], 16).map_err(|_| make_error())?,
            b: u8::from_str_radix(&tag_value[5..7], 16).map_err(|_| make_error())?,
        }))
    }

    fn try_get_bits(&self, tag_key: &'static str) -> Result<Option<u64>, ServerMessageParseError> {
        // this is complicated because we can get:
        // Some(Some(value)) - obvious case, there is a value in the tags (@bits=500)
        // Some(None) - Tag exists, but does not have value (@bits)
        // None - bits key does not exist in tags at all.
        let tag_value = self.try_get_nonempty_tag_value(tag_key)?;

        let bits_amount = u64::from_str(tag_value)
            .map_err(|_| MalformedTagValue(tag_key, tag_value.to_owned()))?;
        Ok(Some(bits_amount))
    }

    fn try_get_timestamp(
        &self,
        tag_key: &'static str,
    ) -> Result<DateTime<Utc>, ServerMessageParseError> {
        // e.g. tmi-sent-ts.
        let tag_value = self.try_get_nonempty_tag_value(tag_key)?;
        let milliseconds_since_epoch = i64::from_str(tag_value)
            .map_err(|_| MalformedTagValue(tag_key, tag_value.to_owned()))?;
        let date = Utc.timestamp_millis(milliseconds_since_epoch);
        Ok(date)
    }
}

// makes it so users cannot match against Generic and get the underlying IRCMessage
// that way (which would break their implementations if there is an enum variant added and they
// expect certain commands to be emitted under Generic)
// that means the only way to get the IRCMessage is via IRCMessage::from()/.into()
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
    Privmsg(PrivmsgMessage),
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
            "PRIVMSG" => Privmsg(PrivmsgMessage::try_from(source)?),
            _ => Generic(HiddenIRCMessage(source)),
        })
    }
}

impl From<ServerMessage> for IRCMessage {
    fn from(msg: ServerMessage) -> IRCMessage {
        match msg {
            ServerMessage::Join(msg) => msg.into(),
            ServerMessage::Part(msg) => msg.into(),
            ServerMessage::Ping(msg) => msg.into(),
            ServerMessage::Pong(msg) => msg.into(),
            ServerMessage::Reconnect(msg) => msg.into(),
            ServerMessage::Privmsg(msg) => msg.into(),
            ServerMessage::Generic(msg) => msg.0,
        }
    }
}
