pub mod clearchat;
pub mod clearmsg;
pub mod globaluserstate;
pub mod hosttarget;
pub mod join;
pub mod notice;
pub mod part;
pub mod ping;
pub mod pong;
pub mod privmsg;
pub mod reconnect;
pub mod roomstate;
pub mod usernotice;
pub mod userstate;
pub mod whisper;
// TODO types: CLEARMSG, ROOMSTATE, USERSTATE, GLOBALUSERSTATE, WHISPER, HOSTTARGET, NOTICE, USERNOTICE

use self::ServerMessageParseError::*;
use crate::message::commands::clearmsg::ClearMsgMessage;
use crate::message::commands::join::JoinMessage;
use crate::message::commands::part::PartMessage;
use crate::message::commands::ping::PingMessage;
use crate::message::commands::pong::PongMessage;
use crate::message::commands::reconnect::ReconnectMessage;
use crate::message::commands::userstate::UserStateMessage;
use crate::message::prefix::IRCPrefix;
use crate::message::twitch::{Badge, Emote, RGBColor};
use crate::message::{
    AsRawIRC, ClearChatMessage, GlobalUserStateMessage, HostTargetMessage, IRCMessage,
    NoticeMessage, PrivmsgMessage, RoomStateMessage, UserNoticeMessage, WhisperMessage,
};
use chrono::{DateTime, TimeZone, Utc};
use itertools::Itertools;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::ops::Range;
use std::str::FromStr;
use thiserror::Error;

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

/// Errors encountered while trying to parse an IRC message as a more specialized "server message",
/// based on its IRC command.
#[derive(Error, Debug, PartialEq)]
pub enum ServerMessageParseError {
    /// That command's data is not parsed by this implementation
    ///
    /// This type of error is only returned if you use `try_from` directly on a special
    /// server message implementation, instead of the general `ServerMessage::try_from`
    /// which covers all implementations and does not emit this type of error.
    #[error("Could not parse IRC message {} as ServerMessage: That command's data is not parsed by this implementation", .0.as_raw_irc())]
    MismatchedCommand(IRCMessage),
    /// No tag present under key `key`
    #[error("Could not parse IRC message {} as ServerMessage: No tag present under key `{1}`", .0.as_raw_irc())]
    MissingTag(IRCMessage, &'static str),
    /// No tag value present under key `key`
    #[error("Could not parse IRC message {} as ServerMessage: No tag value present under key `{1}`", .0.as_raw_irc())]
    MissingTagValue(IRCMessage, &'static str),
    /// Malformed tag value for tag `key`, value was `value`
    #[error("Could not parse IRC message {} as ServerMessage: Malformed tag value for tag `{1}`, value was `{2}`", .0.as_raw_irc())]
    MalformedTagValue(IRCMessage, &'static str, String),
    /// No parameter found at index `n`
    #[error("Could not parse IRC message {} as ServerMessage: No parameter found at index {1}", .0.as_raw_irc())]
    MissingParameter(IRCMessage, usize),
    /// Malformed channel parameter (`#` must be present + something after it)
    #[error("Could not parse IRC message {} as ServerMessage: Malformed channel parameter (# must be present + something after it)", .0.as_raw_irc())]
    MalformedChannel(IRCMessage),
    /// Malformed parameter at index `n`
    #[error("Could not parse IRC message {} as ServerMessage: Malformed parameter at index {1}", .0.as_raw_irc())]
    MalformedParameter(IRCMessage, usize),
    /// Missing prefix altogether
    #[error("Could not parse IRC message {} as ServerMessage: Missing prefix altogether", .0.as_raw_irc())]
    MissingPrefix(IRCMessage),
    /// No nickname found in prefix
    #[error("Could not parse IRC message {} as ServerMessage: No nickname found in prefix", .0.as_raw_irc())]
    MissingNickname(IRCMessage),
}

impl From<ServerMessageParseError> for IRCMessage {
    fn from(msg: ServerMessageParseError) -> IRCMessage {
        match msg {
            ServerMessageParseError::MismatchedCommand(m) => m,
            ServerMessageParseError::MissingTag(m, _) => m,
            ServerMessageParseError::MissingTagValue(m, _) => m,
            ServerMessageParseError::MalformedTagValue(m, _, _) => m,
            ServerMessageParseError::MissingParameter(m, _) => m,
            ServerMessageParseError::MalformedChannel(m) => m,
            ServerMessageParseError::MalformedParameter(m, _) => m,
            ServerMessageParseError::MissingPrefix(m) => m,
            ServerMessageParseError::MissingNickname(m) => m,
        }
    }
}

trait IRCMessageParseExt {
    fn try_get_param(&self, index: usize) -> Result<&str, ServerMessageParseError>;
    fn try_get_message_text(&self) -> Result<(&str, bool), ServerMessageParseError>;
    fn try_get_tag_value(&self, key: &'static str)
        -> Result<Option<&str>, ServerMessageParseError>;
    fn try_get_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<&str, ServerMessageParseError>;
    fn try_get_optional_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<Option<&str>, ServerMessageParseError>;
    fn try_get_channel_login(&self) -> Result<&str, ServerMessageParseError>;
    fn try_get_optional_channel_login(&self) -> Result<Option<&str>, ServerMessageParseError>;
    fn try_get_prefix_nickname(&self) -> Result<&str, ServerMessageParseError>;
    fn try_get_emotes(
        &self,
        tag_key: &'static str,
        message_text: &str,
    ) -> Result<Vec<Emote>, ServerMessageParseError>;
    fn try_get_emote_sets(
        &self,
        tag_key: &'static str,
    ) -> Result<HashSet<String>, ServerMessageParseError>;
    fn try_get_badges(&self, tag_key: &'static str) -> Result<Vec<Badge>, ServerMessageParseError>;
    fn try_get_color(
        &self,
        tag_key: &'static str,
    ) -> Result<Option<RGBColor>, ServerMessageParseError>;
    fn try_get_number<N: FromStr>(
        &self,
        tag_key: &'static str,
    ) -> Result<N, ServerMessageParseError>;
    fn try_get_bool(&self, tag_key: &'static str) -> Result<bool, ServerMessageParseError>;
    fn try_get_optional_number<N: FromStr>(
        &self,
        tag_key: &'static str,
    ) -> Result<Option<N>, ServerMessageParseError>;
    fn try_get_optional_bool(
        &self,
        tag_key: &'static str,
    ) -> Result<Option<bool>, ServerMessageParseError>;
    fn try_get_timestamp(
        &self,
        tag_key: &'static str,
    ) -> Result<DateTime<Utc>, ServerMessageParseError>;
}

impl IRCMessageParseExt for IRCMessage {
    fn try_get_param(&self, index: usize) -> Result<&str, ServerMessageParseError> {
        Ok(self
            .params
            .get(index)
            .ok_or_else(|| MissingParameter(self.to_owned(), index))?)
    }

    fn try_get_message_text(&self) -> Result<(&str, bool), ServerMessageParseError> {
        let mut message_text = self.try_get_param(1)?;

        let is_action =
            message_text.starts_with("\u{0001}ACTION ") && message_text.ends_with('\u{0001}');
        if is_action {
            // remove the prefix and suffix
            message_text = &message_text[8..message_text.len() - 1]
        }

        Ok((message_text, is_action))
    }

    fn try_get_tag_value(
        &self,
        key: &'static str,
    ) -> Result<Option<&str>, ServerMessageParseError> {
        match self.tags.0.get(key) {
            Some(Some(value)) => Ok(Some(value)),
            Some(None) => Ok(None),
            None => Err(MissingTag(self.to_owned(), key)),
        }
    }

    fn try_get_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<&str, ServerMessageParseError> {
        match self.tags.0.get(key) {
            Some(Some(value)) => Ok(value),
            Some(None) => Err(MissingTagValue(self.to_owned(), key)),
            None => Err(MissingTag(self.to_owned(), key)),
        }
    }

    fn try_get_optional_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<Option<&str>, ServerMessageParseError> {
        match self.tags.0.get(key) {
            Some(Some(value)) => Ok(Some(value)),
            Some(None) => Err(MissingTagValue(self.to_owned(), key)),
            None => Ok(None),
        }
    }

    fn try_get_channel_login(&self) -> Result<&str, ServerMessageParseError> {
        let param = self.try_get_param(0)?;

        if !param.starts_with('#') || param.len() < 2 {
            return Err(MalformedChannel(self.to_owned()));
        }

        Ok(&param[1..])
    }

    fn try_get_optional_channel_login(&self) -> Result<Option<&str>, ServerMessageParseError> {
        let param = self.try_get_param(0)?;

        if param == "*" {
            return Ok(None);
        }

        if !param.starts_with('#') || param.len() < 2 {
            return Err(MalformedChannel(self.to_owned()));
        }

        Ok(Some(&param[1..]))
    }

    /// Get the sending user's login name from the IRC prefix.
    fn try_get_prefix_nickname(&self) -> Result<&str, ServerMessageParseError> {
        match &self.prefix {
            None => Err(MissingPrefix(self.to_owned())),
            Some(IRCPrefix::HostOnly { host: _ }) => Err(MissingNickname(self.to_owned())),
            Some(IRCPrefix::Full {
                nick,
                user: _,
                host: _,
            }) => Ok(nick),
        }
    }

    fn try_get_emotes(
        &self,
        tag_key: &'static str,
        message_text: &str,
    ) -> Result<Vec<Emote>, ServerMessageParseError> {
        let tag_value = self.try_get_nonempty_tag_value(tag_key)?;

        if tag_value.is_empty() {
            return Ok(vec![]);
        }

        let mut emotes = Vec::new();

        let make_error = || MalformedTagValue(self.to_owned(), tag_key, tag_value.to_owned());

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

                // we intentionally gracefully handle indices that are out of bounds for the
                // given string by taking as much as possible until the end of the string.
                // This is to work around a Twitch bug: https://github.com/twitchdev/issues/issues/104

                emotes.push(Emote {
                    id: emote_id.to_owned(),
                    char_range: Range { start, end },
                    code,
                });
            }
        }

        emotes.sort_unstable_by_key(|e| e.char_range.start);

        Ok(emotes)
    }

    fn try_get_emote_sets(
        &self,
        tag_key: &'static str,
    ) -> Result<HashSet<String>, ServerMessageParseError> {
        let src = self.try_get_nonempty_tag_value(tag_key)?;

        if src.is_empty() {
            Ok(HashSet::new())
        } else {
            Ok(src.split(",").map(|s| s.to_owned()).collect())
        }
    }

    fn try_get_badges(&self, tag_key: &'static str) -> Result<Vec<Badge>, ServerMessageParseError> {
        // TODO same thing as above, could be optimized to not clone the tag value as well
        let tag_value = self.try_get_nonempty_tag_value(tag_key)?;

        if tag_value.is_empty() {
            return Ok(vec![]);
        }

        let mut badges = Vec::new();

        let make_error = || MalformedTagValue(self.to_owned(), tag_key, tag_value.to_owned());

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
        let make_error = || MalformedTagValue(self.to_owned(), tag_key, tag_value.to_owned());

        if tag_value.is_empty() {
            return Ok(None);
        }

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

    fn try_get_number<N: FromStr>(
        &self,
        tag_key: &'static str,
    ) -> Result<N, ServerMessageParseError> {
        let tag_value = self.try_get_nonempty_tag_value(tag_key)?;
        let number = N::from_str(tag_value)
            .map_err(|_| MalformedTagValue(self.to_owned(), tag_key, tag_value.to_owned()))?;
        Ok(number)
    }

    fn try_get_bool(&self, tag_key: &'static str) -> Result<bool, ServerMessageParseError> {
        Ok(self.try_get_number::<u8>(tag_key)? > 0)
    }

    fn try_get_optional_number<N: FromStr>(
        &self,
        tag_key: &'static str,
    ) -> Result<Option<N>, ServerMessageParseError> {
        let tag_value = match self.tags.0.get(tag_key) {
            Some(Some(value)) => value,
            Some(None) => return Err(MissingTagValue(self.to_owned(), tag_key)),
            None => return Ok(None),
        };

        let number = N::from_str(tag_value)
            .map_err(|_| MalformedTagValue(self.to_owned(), tag_key, tag_value.to_owned()))?;
        Ok(Some(number))
    }

    fn try_get_optional_bool(
        &self,
        tag_key: &'static str,
    ) -> Result<Option<bool>, ServerMessageParseError> {
        Ok(self.try_get_optional_number::<u8>(tag_key)?.map(|n| n > 0))
    }

    fn try_get_timestamp(
        &self,
        tag_key: &'static str,
    ) -> Result<DateTime<Utc>, ServerMessageParseError> {
        // e.g. tmi-sent-ts.
        let tag_value = self.try_get_nonempty_tag_value(tag_key)?;
        let milliseconds_since_epoch = i64::from_str(tag_value)
            .map_err(|_| MalformedTagValue(self.to_owned(), tag_key, tag_value.to_owned()))?;
        Utc.timestamp_millis_opt(milliseconds_since_epoch)
            .single()
            .ok_or_else(|| MalformedTagValue(self.to_owned(), tag_key, tag_value.to_owned()))
    }
}

// makes it so users cannot match against Generic and get the underlying IRCMessage
// that way (which would break their implementations if there is an enum variant added and they
// expect certain commands to be emitted under Generic)
// that means the only way to get the IRCMessage is via IRCMessage::from()/.into()
// which combined with #[non_exhaustive] allows us to add enum variants
// without making a major release
#[derive(Debug, PartialEq, Clone)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
#[doc(hidden)]
pub struct HiddenIRCMessage(pub(self) IRCMessage);

/// An IRCMessage that has been parsed into a more concrete type based on its command.
///
/// This type is non-exhausive, because more types of commands exist and can be added.
///
/// If you wish to (manually) parse a type of command that is not already parsed by this library,
/// use `IRCMessage::from` to convert the `ServerMessage` back to an `IRCMessage`, then
/// check the message's `command` and perform your parsing.
///
/// There is intentionally no generic `Unparsed` variant here. If there was, and the library
/// added parsing for the command you were trying to catch by matching against the `Unparsed`
/// variant, your code would be broken without any compiler error.
///
/// # Examples
///
/// ```
/// use twitch_irc::message::{IRCMessage, ServerMessage};
/// use std::convert::TryFrom;
///
/// let irc_message = IRCMessage::parse(":tmi.twitch.tv PING").unwrap();
/// let server_message = ServerMessage::try_from(irc_message).unwrap();
///
/// match server_message {
///     // match against known types first
///     ServerMessage::Ping { .. } => println!("Got pinged!"),
///     rest => {
///         // can do manual parsing here
///         let irc_message = IRCMessage::from(rest);
///         if irc_message.command == "CUSTOMCMD" {
///              // ...
///         }
///     }
/// }
/// ```
#[derive(Debug, Clone)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
#[non_exhaustive]
pub enum ServerMessage {
    /// `CLEARCHAT` message
    ClearChat(ClearChatMessage),
    /// `CLEARMSG` message
    ClearMsg(ClearMsgMessage),
    /// `GLOBALUSERSTATE` message
    GlobalUserState(GlobalUserStateMessage),
    /// `HOSTTARGET` message
    HostTarget(HostTargetMessage),
    /// `JOIN` message
    Join(JoinMessage),
    /// `NOTICE` message
    Notice(NoticeMessage),
    /// `PART` message
    Part(PartMessage),
    /// `PING` message
    Ping(PingMessage),
    /// `PONG` message
    Pong(PongMessage),
    /// `PRIVMSG` message
    Privmsg(PrivmsgMessage),
    /// `RECONNECT` message
    Reconnect(ReconnectMessage),
    /// `ROOMSTATE` message
    RoomState(RoomStateMessage),
    /// `USERNOTICE` message
    UserNotice(UserNoticeMessage),
    /// `USERSTATE` message
    UserState(UserStateMessage),
    /// `WHISPER` message
    Whisper(WhisperMessage),
    #[doc(hidden)]
    Generic(HiddenIRCMessage),
}

impl TryFrom<IRCMessage> for ServerMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<ServerMessage, ServerMessageParseError> {
        use ServerMessage::*;

        Ok(match source.command.as_str() {
            "CLEARCHAT" => ClearChat(ClearChatMessage::try_from(source)?),
            "CLEARMSG" => ClearMsg(ClearMsgMessage::try_from(source)?),
            "GLOBALUSERSTATE" => GlobalUserState(GlobalUserStateMessage::try_from(source)?),
            "HOSTTARGET" => HostTarget(HostTargetMessage::try_from(source)?),
            "JOIN" => Join(JoinMessage::try_from(source)?),
            "NOTICE" => Notice(NoticeMessage::try_from(source)?),
            "PART" => Part(PartMessage::try_from(source)?),
            "PING" => Ping(PingMessage::try_from(source)?),
            "PONG" => Pong(PongMessage::try_from(source)?),
            "PRIVMSG" => Privmsg(PrivmsgMessage::try_from(source)?),
            "RECONNECT" => Reconnect(ReconnectMessage::try_from(source)?),
            "ROOMSTATE" => RoomState(RoomStateMessage::try_from(source)?),
            "USERNOTICE" => UserNotice(UserNoticeMessage::try_from(source)?),
            "USERSTATE" => UserState(UserStateMessage::try_from(source)?),
            "WHISPER" => Whisper(WhisperMessage::try_from(source)?),
            _ => Generic(HiddenIRCMessage(source)),
        })
    }
}

impl From<ServerMessage> for IRCMessage {
    fn from(msg: ServerMessage) -> IRCMessage {
        match msg {
            ServerMessage::ClearChat(msg) => msg.source,
            ServerMessage::ClearMsg(msg) => msg.source,
            ServerMessage::GlobalUserState(msg) => msg.source,
            ServerMessage::HostTarget(msg) => msg.source,
            ServerMessage::Join(msg) => msg.source,
            ServerMessage::Notice(msg) => msg.source,
            ServerMessage::Part(msg) => msg.source,
            ServerMessage::Ping(msg) => msg.source,
            ServerMessage::Pong(msg) => msg.source,
            ServerMessage::Privmsg(msg) => msg.source,
            ServerMessage::Reconnect(msg) => msg.source,
            ServerMessage::RoomState(msg) => msg.source,
            ServerMessage::UserNotice(msg) => msg.source,
            ServerMessage::UserState(msg) => msg.source,
            ServerMessage::Whisper(msg) => msg.source,
            ServerMessage::Generic(msg) => msg.0,
        }
    }
}

// borrowed variant of the above
impl ServerMessage {
    /// Get a reference to the `IRCMessage` this `ServerMessage` was parsed from.
    pub fn source(&self) -> &IRCMessage {
        match self {
            ServerMessage::ClearChat(msg) => &msg.source,
            ServerMessage::ClearMsg(msg) => &msg.source,
            ServerMessage::GlobalUserState(msg) => &msg.source,
            ServerMessage::HostTarget(msg) => &msg.source,
            ServerMessage::Join(msg) => &msg.source,
            ServerMessage::Notice(msg) => &msg.source,
            ServerMessage::Part(msg) => &msg.source,
            ServerMessage::Ping(msg) => &msg.source,
            ServerMessage::Pong(msg) => &msg.source,
            ServerMessage::Privmsg(msg) => &msg.source,
            ServerMessage::Reconnect(msg) => &msg.source,
            ServerMessage::RoomState(msg) => &msg.source,
            ServerMessage::UserNotice(msg) => &msg.source,
            ServerMessage::UserState(msg) => &msg.source,
            ServerMessage::Whisper(msg) => &msg.source,
            ServerMessage::Generic(msg) => &msg.0,
        }
    }

    pub(crate) fn new_generic(message: IRCMessage) -> ServerMessage {
        ServerMessage::Generic(HiddenIRCMessage(message))
    }
}

impl AsRawIRC for ServerMessage {
    fn format_as_raw_irc(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.source().format_as_raw_irc(f)
    }
}
