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
use smallvec::alloc::fmt::Formatter;
use std::collections::HashSet;
use std::convert::TryFrom;
use std::ops::Range;
use std::str::FromStr;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
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
    #[error("Malformed parameter at index {0}")]
    MalformedParameter(usize),
    #[error("Missing prefix altogether")]
    MissingPrefix(),
    #[error("No nickname found in prefix")]
    MissingNickname(),
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
    ) -> Result<HashSet<u64>, ServerMessageParseError>;
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
        Ok(self.params.get(index).ok_or(MissingParameter(index))?)
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
            None => Err(MissingTag(key)),
        }
    }

    fn try_get_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<&str, ServerMessageParseError> {
        match self.tags.0.get(key) {
            Some(Some(value)) => Ok(value),
            Some(None) => Err(MissingTagValue(key)),
            None => Err(MissingTag(key)),
        }
    }

    fn try_get_optional_nonempty_tag_value(
        &self,
        key: &'static str,
    ) -> Result<Option<&str>, ServerMessageParseError> {
        match self.tags.0.get(key) {
            Some(Some(value)) => Ok(Some(value)),
            Some(None) => Err(MissingTagValue(key)),
            None => Ok(None),
        }
    }

    fn try_get_channel_login(&self) -> Result<&str, ServerMessageParseError> {
        let param = self.try_get_param(0)?;

        if !param.starts_with('#') || param.len() < 2 {
            return Err(MalformedChannel());
        }

        Ok(&param[1..])
    }

    fn try_get_optional_channel_login(&self) -> Result<Option<&str>, ServerMessageParseError> {
        let param = self.try_get_param(0)?;

        if param == "*" {
            return Ok(None);
        }

        if !param.starts_with('#') || param.len() < 2 {
            return Err(MalformedChannel());
        }

        Ok(Some(&param[1..]))
    }

    /// Get the sending user's login name from the IRC prefix.
    fn try_get_prefix_nickname(&self) -> Result<&str, ServerMessageParseError> {
        match &self.prefix {
            None => Err(MissingPrefix()),
            Some(IRCPrefix::HostOnly { host: _ }) => Err(MissingNickname()),
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

        emotes.sort_unstable_by_key(|e| e.char_range.start);

        Ok(emotes)
    }

    fn try_get_emote_sets(
        &self,
        tag_key: &'static str,
    ) -> Result<HashSet<u64>, ServerMessageParseError> {
        let src = self.try_get_nonempty_tag_value(tag_key)?;

        if src == "" {
            Ok(HashSet::new())
        } else {
            let mut emote_sets = HashSet::new();

            for emote_set in src.split(',') {
                emote_sets.insert(
                    u64::from_str(&emote_set)
                        .map_err(|_| MalformedTagValue(tag_key, src.to_owned()))?,
                );
            }

            Ok(emote_sets)
        }
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

        if tag_value == "" {
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
        let number =
            N::from_str(tag_value).map_err(|_| MalformedTagValue(tag_key, tag_value.to_owned()))?;
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
            Some(None) => return Err(MissingTagValue(tag_key)),
            None => return Ok(None),
        };

        let number =
            N::from_str(tag_value).map_err(|_| MalformedTagValue(tag_key, tag_value.to_owned()))?;
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
            .map_err(|_| MalformedTagValue(tag_key, tag_value.to_owned()))?;
        Utc.timestamp_millis_opt(milliseconds_since_epoch)
            .single()
            .ok_or_else(|| MalformedTagValue(tag_key, tag_value.to_owned()))
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
#[non_exhaustive]
pub enum ServerMessage {
    ClearChat(ClearChatMessage),
    ClearMsg(ClearMsgMessage),
    GlobalUserState(GlobalUserStateMessage),
    HostTarget(HostTargetMessage),
    Join(JoinMessage),
    Notice(NoticeMessage),
    Part(PartMessage),
    Ping(PingMessage),
    Pong(PongMessage),
    Privmsg(PrivmsgMessage),
    Reconnect(ReconnectMessage),
    RoomState(RoomStateMessage),
    UserNotice(UserNoticeMessage),
    UserState(UserStateMessage),
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
}

impl AsRawIRC for ServerMessage {
    fn format_as_raw_irc(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.source().format_as_raw_irc(f)
    }
}
