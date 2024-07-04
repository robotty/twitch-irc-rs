//! Generic and Twitch-specific IRC messages.

pub(crate) mod commands;
pub(crate) mod prefix;
pub(crate) mod tags;
pub(crate) mod twitch;

pub use commands::clearchat::{ClearChatAction, ClearChatMessage};
pub use commands::clearmsg::ClearMsgMessage;
pub use commands::globaluserstate::GlobalUserStateMessage;
pub use commands::join::JoinMessage;
pub use commands::notice::NoticeMessage;
pub use commands::part::PartMessage;
pub use commands::ping::PingMessage;
pub use commands::pong::PongMessage;
pub use commands::privmsg::PrivmsgMessage;
pub use commands::reconnect::ReconnectMessage;
pub use commands::roomstate::{FollowersOnlyMode, RoomStateMessage};
pub use commands::usernotice::{SubGiftPromo, UserNoticeEvent, UserNoticeMessage};
pub use commands::userstate::UserStateMessage;
pub use commands::whisper::WhisperMessage;
pub use commands::{ServerMessage, ServerMessageParseError};
use fast_str::FastStr;
pub use prefix::IRCPrefix;
pub use tags::IRCTags;
pub use twitch::*;

use std::fmt;
use std::fmt::Write;
use thiserror::Error;

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

/// Error while parsing a FastStr into an `IRCMessage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum IRCParseError {
    /// No space found after tags (no command/prefix)
    #[error("No space found after tags (no command/prefix)")]
    NoSpaceAfterTags,
    /// No tags after @ sign
    #[error("No tags after @ sign")]
    EmptyTagsDeclaration,
    /// No space found after prefix (no command)
    #[error("No space found after prefix (no command)")]
    NoSpaceAfterPrefix,
    /// No tags after : sign
    #[error("No tags after : sign")]
    EmptyPrefixDeclaration,
    /// Expected command to only consist of alphabetic or numeric characters
    #[error("Expected command to only consist of alphabetic or numeric characters")]
    MalformedCommand,
    /// Expected only single spaces between middle parameters
    #[error("Expected only single spaces between middle parameters")]
    TooManySpacesInMiddleParams,
    /// Newlines are not permitted in raw IRC messages
    #[error("Newlines are not permitted in raw IRC messages")]
    NewlinesInMessage,
}

struct RawIRCDisplay<'a, T: AsRawIRC>(&'a T);

impl<'a, T: AsRawIRC> fmt::Display for RawIRCDisplay<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.format_as_raw_irc(f)
    }
}

/// Anything that can be converted into the raw IRC wire format.
pub trait AsRawIRC {
    /// Writes the raw IRC message to the given formatter.
    fn format_as_raw_irc(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    /// Creates a new FastStr with the raw IRC message.
    ///
    /// The resulting output FastStr is guaranteed to parse to the same value it was created from,
    /// but due to protocol ambiguity it is not guaranteed to be identical to the input
    /// the value was parsed from (if it was parsed at all).
    ///
    /// For example, the order of tags might differ, or the use of trailing parameters
    /// might be different.
    fn as_raw_irc(&self) -> FastStr
    where
        Self: Sized,
    {
        FastStr::from_string(format!("{}", RawIRCDisplay(self)))
    }
}

/// A protocol-level IRC message, with arbitrary command, parameters, tags and prefix.
///
/// See [RFC 2812, section 2.3.1](https://tools.ietf.org/html/rfc2812#section-2.3.1)
/// for the message format that this is based on.
/// Further, this implements [IRCv3 tags](https://ircv3.net/specs/extensions/message-tags.html).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "with-serde",
    derive(
        Serialize,
        Deserialize
    )
)]
pub struct IRCMessage {
    /// A map of additional key-value tags on this message.
    pub tags: IRCTags,
    /// The "prefix" of this message, as defined by RFC 2812. Typically specifies the sending
    /// server and/or user.
    pub prefix: Option<IRCPrefix>,
    /// A command like `PRIVMSG` or `001` (see RFC 2812 for the definition).
    pub command: FastStr,
    /// A list of parameters on this IRC message. See RFC 2812 for the definition.
    ///
    /// Middle parameters and trailing parameters are treated the same here, and as long as
    /// there are no spaces in the last parameter, there is no way to tell if that parameter
    /// was a middle or trailing parameter when it was parsed.
    pub params: Vec<FastStr>,
}

/// Allows quick creation of simple IRC messages using a command and optional parameters.
///
/// The given command and parameters have to implement `From<T> for FastStr` if they are not
/// already of type `FastStr`.
///
/// # Example
///
/// ```
/// use twitch_irc::irc;
/// use twitch_irc::message::AsRawIRC;
///
/// # fn main() {
/// let msg = irc!["PRIVMSG", "#sodapoppin", "Hello guys!"];
///
/// assert_eq!(msg.command, "PRIVMSG");
/// assert_eq!(msg.params, vec!["#sodapoppin".into(), "Hello guys!".into()]);
/// assert_eq!(msg.as_raw_irc(), "PRIVMSG #sodapoppin :Hello guys!");
/// # }
/// ```
#[macro_export]
macro_rules! irc {
    (@replace_expr $_t:tt $sub:expr) => {
        $sub
    };
    (@count_exprs $($expression:expr),*) => {
        0usize $(+ irc!(@replace_expr $expression 1usize))*
    };
    ($command:expr $(, $argument:expr )* ) => {
        {
            let capacity = irc!(@count_exprs $($argument),*);
            #[allow(unused_mut)]
            let mut temp_vec: Vec<fast_str::FastStr> = ::std::vec::Vec::with_capacity(capacity);
            $(
                temp_vec.push(fast_str::FastStr::from($argument));
            )*
            $crate::message::IRCMessage::new_simple(fast_str::FastStr::from($command), temp_vec)
        }
    };
}

impl IRCMessage {
    /// Create a new `IRCMessage` with just a command and parameters, similar to the
    /// `irc!` macro.
    pub fn new_simple(command: FastStr, params: Vec<FastStr>) -> IRCMessage {
        IRCMessage {
            tags: IRCTags::new(),
            prefix: None,
            command,
            params,
        }
    }

    /// Create a new `IRCMessage` by specifying all fields.
    pub fn new(
        tags: IRCTags,
        prefix: Option<IRCPrefix>,
        command: FastStr,
        params: Vec<FastStr>,
    ) -> IRCMessage {
        IRCMessage {
            tags,
            prefix,
            command,
            params,
        }
    }

    /// Parse a raw IRC wire-format message into an `IRCMessage`. `source` should be specified
    /// without trailing newline character(s).
    pub fn parse(mut source: &str) -> Result<IRCMessage, IRCParseError> {
        if source.chars().any(|c| c == '\r' || c == '\n') {
            return Err(IRCParseError::NewlinesInMessage);
        }

        let tags = if source.starts_with('@') {
            // str[1..] removes the leading @ sign
            let (tags_part, remainder) = source[1..]
                .split_once(' ')
                .ok_or(IRCParseError::NoSpaceAfterTags)?;
            source = remainder;

            if tags_part.is_empty() {
                return Err(IRCParseError::EmptyTagsDeclaration);
            }

            IRCTags::parse(tags_part)
        } else {
            IRCTags::new()
        };

        let prefix = if source.starts_with(':') {
            // str[1..] removes the leading : sign
            let (prefix_part, remainder) = source[1..]
                .split_once(' ')
                .ok_or(IRCParseError::NoSpaceAfterPrefix)?;
            source = remainder;

            if prefix_part.is_empty() {
                return Err(IRCParseError::EmptyPrefixDeclaration);
            }

            Some(IRCPrefix::parse(prefix_part))
        } else {
            None
        };

        let mut command_split = source.splitn(2, ' ');
        let mut command = command_split.next().unwrap().to_owned();

        command.make_ascii_uppercase();

        let command = FastStr::from_string(command);

        if command.is_empty()
            || !command.chars().all(|c| c.is_ascii_alphabetic())
                && !command.chars().all(|c| c.is_ascii() && c.is_numeric())
        {
            return Err(IRCParseError::MalformedCommand);
        }

        let mut params;
        if let Some(params_part) = command_split.next() {
            params = vec![];

            let mut rest = Some(params_part);
            while let Some(rest_str) = rest {
                if let Some(sub_str) = rest_str.strip_prefix(':') {
                    // trailing param, remove : and consume the rest of the input
                    params.push(FastStr::from_ref(sub_str));
                    rest = None;
                } else {
                    let mut split = rest_str.splitn(2, ' ');
                    let param = split.next().unwrap();
                    rest = split.next();

                    if param.is_empty() {
                        return Err(IRCParseError::TooManySpacesInMiddleParams);
                    }
                    params.push(FastStr::from_ref(param));
                }
            }
        } else {
            params = vec![];
        };

        Ok(IRCMessage {
            tags,
            prefix,
            command,
            params,
        })
    }
}

impl AsRawIRC for IRCMessage {
    fn format_as_raw_irc(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.tags.0.is_empty() {
            f.write_char('@')?;
            self.tags.format_as_raw_irc(f)?;
            f.write_char(' ')?;
        }

        if let Some(prefix) = &self.prefix {
            f.write_char(':')?;
            prefix.format_as_raw_irc(f)?;
            f.write_char(' ')?;
        }

        f.write_str(&self.command)?;

        for param in self.params.iter() {
            if !param.contains(' ') && !param.is_empty() && !param.starts_with(':') {
                // middle parameter
                write!(f, " {}", param)?;
            } else {
                // trailing parameter
                write!(f, " :{}", param)?;
                // TODO should there be a panic if this is not the last parameter?
                break;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashmap;

    #[test]
    fn test_privmsg() {
        let source = "@rm-received-ts=1577040815136;historical=1;badge-info=subscriber/16;badges=moderator/1,subscriber/12;color=#19E6E6;display-name=randers;emotes=;flags=;id=6e2ccb1f-01ed-44d0-85b6-edf762524475;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1577040814959;turbo=0;user-id=40286300;user-type=mod :randers!randers@randers.tmi.twitch.tv PRIVMSG #pajlada :Pajapains";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "display-name".into() => "randers".into(),
                    "tmi-sent-ts" .into() => "1577040814959".into(),
                    "historical".into() => "1".into(),
                    "room-id".into() => "11148817".into(),
                    "emotes".into() => "".into(),
                    "color".into() => "#19E6E6".into(),
                    "id".into() => "6e2ccb1f-01ed-44d0-85b6-edf762524475".into(),
                    "turbo".into() => "0".into(),
                    "flags".into() => "".into(),
                    "user-id".into() => "40286300".into(),
                    "rm-received-ts".into() => "1577040815136".into(),
                    "user-type".into() => "mod".into(),
                    "subscriber".into() => "1".into(),
                    "badges".into() => "moderator/1,subscriber/12".into(),
                    "badge-info".into() => "subscriber/16".into(),
                    "mod".into() => "1".into(),
                }),
                prefix: Some(IRCPrefix::Full {
                    nick: "randers".into(),
                    user: Some("randers".into()),
                    host: Some("randers.tmi.twitch.tv".into()),
                }),
                command: "PRIVMSG".into(),
                params: vec!["#pajlada".into(), "Pajapains".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_confusing_prefix_trailing_param() {
        let source = ":coolguy foo bar baz asdf";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".into()
                }),
                command: "FOO".into(),
                params: vec!["bar".into(), "baz".into(), "asdf".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_1() {
        let source = "foo bar baz ::asdf";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "FOO".into(),
                params: vec!["bar".into(), "baz".into(), ":asdf".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_2() {
        let source = ":coolguy foo bar baz :  asdf quux ";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".into()
                }),
                command: "FOO".into(),
                params: vec!["bar".into(), "baz".into(), "  asdf quux ".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_3() {
        let source = ":coolguy PRIVMSG bar :lol :) ";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".into()
                }),
                command: "PRIVMSG".into(),
                params: vec!["bar".into(), "lol :) ".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_4() {
        let source = ":coolguy foo bar baz :";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".into()
                }),
                command: "FOO".into(),
                params: vec!["bar".into(), "baz".into(), "".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_5() {
        let source = ":coolguy foo bar baz :  ";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".into()
                }),
                command: "FOO".into(),
                params: vec!["bar".into(), "baz".into(), "  ".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_6() {
        let source = "@a=b;c=32;k;rt=ql7 foo";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "a".into() => "b".into(),
                    "c".into() => "32".into(),
                    "k".into() => "".into(),
                    "rt".into() => "ql7".into()
                }),
                prefix: None,
                command: "FOO".into(),
                params: vec![],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_7() {
        let source = "@a=b\\\\and\\nk;c=72\\s45;d=gh\\:764 foo";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "a".into() => "b\\and\nk".into(),
                    "c".into() => "72 45".into(),
                    "d".into() => "gh;764".into(),
                }),
                prefix: None,
                command: "FOO".into(),
                params: vec![],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_8() {
        let source = "@c;h=;a=b :quux ab cd";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "c".into() => "".into(),
                    "h".into() => "".into(),
                    "a".into() => "b".into(),
                }),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "quux".into()
                }),
                command: "AB".into(),
                params: vec!["cd".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_join_1() {
        let source = ":src JOIN #chan";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly { host: "src".into() }),
                command: "JOIN".into(),
                params: vec!["#chan".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_join_2() {
        assert_eq!(
            IRCMessage::parse(":src JOIN #chan"),
            IRCMessage::parse(":src JOIN :#chan"),
        )
    }

    #[test]
    fn test_away_1() {
        let source = ":src AWAY";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly { host: "src".into() }),
                command: "AWAY".into(),
                params: vec![],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_away_2() {
        let source = ":cool\tguy foo bar baz";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "cool\tguy".into()
                }),
                command: "FOO".into(),
                params: vec!["bar".into(), "baz".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_complex_prefix() {
        let source = ":coolguy!~ag@n\u{0002}et\u{0003}05w\u{000f}ork.admin PRIVMSG foo :bar baz";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::Full {
                    nick: "coolguy".into(),
                    user: Some("~ag".into()),
                    host: Some("n\u{0002}et\u{0003}05w\u{000f}ork.admin".into())
                }),
                command: "PRIVMSG".into(),
                params: vec!["foo".into(), "bar baz".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_vendor_tags() {
        let source = "@tag1=value1;tag2;vendor1/tag3=value2;vendor2/tag4 :irc.example.com COMMAND param1 param2 :param3 param3";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "tag1".into() => "value1".into(),
                    "tag2".into() => "".into(),
                    "vendor1/tag3".into() => "value2".into(),
                    "vendor2/tag4".into() => "".into()
                }),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "irc.example.com".into()
                }),
                command: "COMMAND".into(),
                params: vec!["param1".into(), "param2".into(), "param3 param3".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_asian_characters_display_name() {
        let source = "@display-name=테스트계정420 :tmi.twitch.tv PRIVMSG #pajlada :test";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "display-name".into() => "테스트계정420".into(),
                }),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "tmi.twitch.tv".into()
                }),
                command: "PRIVMSG".into(),
                params: vec!["#pajlada".into(), "test".into(),],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_ping_1() {
        let source = "PING :tmi.twitch.tv";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "PING".into(),
                params: vec!["tmi.twitch.tv".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_ping_2() {
        let source = ":tmi.twitch.tv PING";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "tmi.twitch.tv".into()
                }),
                command: "PING".into(),
                params: vec![],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_invalid_empty_tags() {
        let result = IRCMessage::parse("@ :tmi.twitch.tv TEST");
        assert_eq!(result, Err(IRCParseError::EmptyTagsDeclaration))
    }

    #[test]
    fn test_invalid_nothing_after_tags() {
        let result = IRCMessage::parse("@key=value");
        assert_eq!(result, Err(IRCParseError::NoSpaceAfterTags))
    }

    #[test]
    fn test_invalid_empty_prefix() {
        let result = IRCMessage::parse("@key=value : TEST");
        assert_eq!(result, Err(IRCParseError::EmptyPrefixDeclaration))
    }

    #[test]
    fn test_invalid_nothing_after_prefix() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv");
        assert_eq!(result, Err(IRCParseError::NoSpaceAfterPrefix))
    }

    #[test]
    fn test_invalid_spaces_at_start_of_line() {
        let result = IRCMessage::parse(" @key=value :tmi.twitch.tv PING");
        assert_eq!(result, Err(IRCParseError::MalformedCommand))
    }

    #[test]
    fn test_invalid_empty_command_1() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv ");
        assert_eq!(result, Err(IRCParseError::MalformedCommand))
    }

    #[test]
    fn test_invalid_empty_command_2() {
        let result = IRCMessage::parse("");
        assert_eq!(result, Err(IRCParseError::MalformedCommand))
    }

    #[test]
    fn test_invalid_command_1() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv  PING");
        assert_eq!(result, Err(IRCParseError::MalformedCommand))
    }

    #[test]
    fn test_invalid_command_2() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv P!NG");
        assert_eq!(result, Err(IRCParseError::MalformedCommand))
    }

    #[test]
    fn test_invalid_command_3() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PØNG");
        assert_eq!(result, Err(IRCParseError::MalformedCommand))
    }

    #[test]
    fn test_invalid_command_4() {
        // mix of ascii numeric and ascii alphabetic
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv P1NG");
        assert_eq!(result, Err(IRCParseError::MalformedCommand))
    }

    #[test]
    fn test_invalid_middle_params_space_after_command() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING ");
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams))
    }

    #[test]
    fn test_invalid_middle_params_too_many_spaces_between_params() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING asd  def");
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams))
    }

    #[test]
    fn test_invalid_middle_params_too_many_spaces_after_command() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING  asd def");
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams))
    }

    #[test]
    fn test_invalid_middle_params_trailing_space() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING asd def ");
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams))
    }

    #[test]
    fn test_empty_trailing_param_1() {
        let source = "PING asd def :";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "PING".into(),
                params: vec!["asd".into(), "def".into(), "".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_empty_trailing_param_2() {
        let source = "PING :";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "PING".into(),
                params: vec!["".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_numeric_command() {
        let source = "500 :Internal Server Error";
        let message = IRCMessage::parse(source).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "500".into(),
                params: vec!["Internal Server Error".into()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_stringify_pass() {
        assert_eq!(
            irc!["PASS", "oauth:9892879487293847"].as_raw_irc(),
            "PASS oauth:9892879487293847"
        );
    }

    #[test]
    fn test_newline_in_source() {
        assert_eq!(
            IRCMessage::parse("abc\ndef"),
            Err(IRCParseError::NewlinesInMessage)
        );
        assert_eq!(
            IRCMessage::parse("abc\rdef"),
            Err(IRCParseError::NewlinesInMessage)
        );
        assert_eq!(
            IRCMessage::parse("abc\n\rdef"),
            Err(IRCParseError::NewlinesInMessage)
        );
    }

    #[test]
    fn test_lowercase_command() {
        assert_eq!(IRCMessage::parse("ping").unwrap().command, "PING")
    }

    #[test]
    fn test_irc_macro() {
        assert_eq!(
            irc!["PRIVMSG"],
            IRCMessage {
                tags: IRCTags::new(),
                prefix: None,
                command: "PRIVMSG".into(),
                params: vec![],
            }
        );
        assert_eq!(
            irc!["PRIVMSG", "#pajlada"],
            IRCMessage {
                tags: IRCTags::new(),
                prefix: None,
                command: "PRIVMSG".into(),
                params: vec!["#pajlada".into()],
            }
        );
        assert_eq!(
            irc!["PRIVMSG", "#pajlada", "LUL xD"],
            IRCMessage {
                tags: IRCTags::new(),
                prefix: None,
                command: "PRIVMSG".into(),
                params: vec!["#pajlada".into(), "LUL xD".into()],
            }
        );
    }
}
