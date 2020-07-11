pub mod commands;
pub mod prefix;
pub mod tags;

use self::prefix::IRCPrefix;
use self::tags::IRCTags;
use derivative::Derivative;
use itertools::Itertools;
use std::fmt;
use std::fmt::Write;
use thiserror::Error;

#[derive(Debug, PartialEq, Error)]
pub enum IRCParseError {
    #[error("no space found after tags (no command/prefix)")]
    NoSpaceAfterTags(),
    #[error("no tags after @ sign")]
    EmptyTagsDeclaration(),
    #[error("no space found after prefix (no command)")]
    NoSpaceAfterPrefix(),
    #[error("no tags after : sign")]
    EmptyPrefixDeclaration(),
    #[error("expected command to only consist of alphabetic or numeric characters")]
    MalformedCommand(),
    #[error("expected only single spaces between middle parameters")]
    TooManySpacesInMiddleParams(),
    #[error("Newlines are not permitted in raw IRC messages")]
    NewlinesInMessage(),
}

struct RawIRCDisplay<'a, T: AsRawIRC> {
    delegate: &'a T,
}

impl<'a, T: AsRawIRC> RawIRCDisplay<'a, T> {
    fn new(delegate: &'a T) -> RawIRCDisplay<'a, T> {
        RawIRCDisplay { delegate }
    }
}

impl<'a, T: AsRawIRC> fmt::Display for RawIRCDisplay<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.delegate.format_as_raw_irc(f)
    }
}

pub trait AsRawIRC {
    fn format_as_raw_irc(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;
    fn as_raw_irc(&self) -> String
    where
        Self: Sized,
    {
        format!("{}", RawIRCDisplay::new(self))
    }
}

#[derive(Derivative, Debug, Clone)]
#[derivative(PartialEq)]
pub struct IRCMessage {
    pub tags: IRCTags,
    pub prefix: Option<IRCPrefix>,
    pub command: String,
    pub params: Vec<String>,

    #[derivative(PartialEq = "ignore")]
    source: Option<String>,
}

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
            let mut temp_vec: Vec<String> = Vec::with_capacity(capacity);
            $(
                temp_vec.push(String::from($argument));
            )*
            crate::message::IRCMessage::new_simple(String::from($command), temp_vec)
        }
    };
}

impl IRCMessage {
    pub fn new_simple(command: String, params: Vec<String>) -> IRCMessage {
        IRCMessage {
            tags: IRCTags::new(),
            prefix: None,
            command,
            params,
            source: None,
        }
    }

    pub fn new(
        tags: IRCTags,
        prefix: Option<IRCPrefix>,
        command: String,
        params: Vec<String>,
    ) -> IRCMessage {
        IRCMessage {
            tags,
            prefix,
            command,
            params,
            source: None,
        }
    }

    pub fn parse(source: String) -> Result<IRCMessage, IRCParseError> {
        let mut str = source.as_str();

        if str.chars().any(|c| c == '\r' || c == '\n') {
            return Err(IRCParseError::NewlinesInMessage());
        }

        let tags = if str.starts_with('@') {
            // str[1..] removes the leading @ sign
            let (tags_part, remainder) = str[1..]
                .splitn(2, ' ')
                .next_tuple()
                .ok_or_else(IRCParseError::NoSpaceAfterTags)?;
            str = remainder;

            if tags_part.is_empty() {
                return Err(IRCParseError::EmptyTagsDeclaration());
            }

            IRCTags::parse(tags_part)
        } else {
            IRCTags::new()
        };

        let prefix = if str.starts_with(':') {
            // str[1..] removes the leading : sign
            let (prefix_part, remainder) = str[1..]
                .splitn(2, ' ')
                .next_tuple()
                .ok_or_else(IRCParseError::NoSpaceAfterPrefix)?;
            str = remainder;

            if prefix_part.is_empty() {
                return Err(IRCParseError::EmptyPrefixDeclaration());
            }

            Some(IRCPrefix::parse(prefix_part))
        } else {
            None
        };

        let mut command_split = str.splitn(2, ' ');
        let mut command = command_split.next().unwrap().to_owned();
        command.make_ascii_uppercase();

        if command.is_empty()
            || !command.chars().all(|c| c.is_ascii_alphabetic())
                && !command.chars().all(|c| c.is_ascii() && c.is_numeric())
        {
            return Err(IRCParseError::MalformedCommand());
        }

        let mut params;
        if let Some(params_part) = command_split.next() {
            params = vec![];

            let mut rest = Some(params_part);
            while let Some(rest_str) = rest {
                if rest_str.starts_with(':') {
                    // trailing param, remove : and consume the rest of the input
                    params.push(rest_str[1..].to_owned());
                    rest = None;
                } else {
                    let mut split = rest_str.splitn(2, ' ');
                    let param = split.next().unwrap();
                    rest = split.next();

                    if param.is_empty() {
                        return Err(IRCParseError::TooManySpacesInMiddleParams());
                    }
                    params.push(param.to_owned());
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
            source: Some(source),
        })
    }
}

impl AsRawIRC for IRCMessage {
    fn format_as_raw_irc(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(source) = &self.source {
            f.write_str(source)?;
            return Ok(());
        }

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
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "display-name".to_owned() => Some("randers".to_owned()),
                    "tmi-sent-ts" .to_owned() => Some("1577040814959".to_owned()),
                    "historical".to_owned() => Some("1".to_owned()),
                    "room-id".to_owned() => Some("11148817".to_owned()),
                    "emotes".to_owned() => Some("".to_owned()),
                    "color".to_owned() => Some("#19E6E6".to_owned()),
                    "id".to_owned() => Some("6e2ccb1f-01ed-44d0-85b6-edf762524475".to_owned()),
                    "turbo".to_owned() => Some("0".to_owned()),
                    "flags".to_owned() => Some("".to_owned()),
                    "user-id".to_owned() => Some("40286300".to_owned()),
                    "rm-received-ts".to_owned() => Some("1577040815136".to_owned()),
                    "user-type".to_owned() => Some("mod".to_owned()),
                    "subscriber".to_owned() => Some("1".to_owned()),
                    "badges".to_owned() => Some("moderator/1,subscriber/12".to_owned()),
                    "badge-info".to_owned() => Some("subscriber/16".to_owned()),
                    "mod".to_owned() => Some("1".to_owned()),
                }),
                prefix: Some(IRCPrefix::Full {
                    nick: "randers".to_owned(),
                    user: Some("randers".to_owned()),
                    host: Some("randers.tmi.twitch.tv".to_owned()),
                }),
                command: "PRIVMSG".to_owned(),
                params: vec!["#pajlada".to_owned(), "Pajapains".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_confusing_prefix_trailing_param() {
        let source = ":coolguy foo bar baz asdf";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "FOO".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned(), "asdf".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_pure_irc_1() {
        let source = "foo bar baz ::asdf";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "FOO".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned(), ":asdf".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_pure_irc_2() {
        let source = ":coolguy foo bar baz :  asdf quux ";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "FOO".to_owned(),
                params: vec![
                    "bar".to_owned(),
                    "baz".to_owned(),
                    "  asdf quux ".to_owned()
                ],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_pure_irc_3() {
        let source = ":coolguy PRIVMSG bar :lol :) ";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "PRIVMSG".to_owned(),
                params: vec!["bar".to_owned(), "lol :) ".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_pure_irc_4() {
        let source = ":coolguy foo bar baz :";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "FOO".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned(), "".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_pure_irc_5() {
        let source = ":coolguy foo bar baz :  ";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "FOO".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned(), "  ".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_pure_irc_6() {
        let source = "@a=b;c=32;k;rt=ql7 foo";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "a".to_owned() => Some("b".to_owned()),
                    "c".to_owned() => Some("32".to_owned()),
                    "k".to_owned() => None,
                    "rt".to_owned() => Some("ql7".to_owned())
                }),
                prefix: None,
                command: "FOO".to_owned(),
                params: vec![],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_pure_irc_7() {
        let source = "@a=b\\\\and\\nk;c=72\\s45;d=gh\\:764 foo";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "a".to_owned() => Some("b\\and\nk".to_owned()),
                    "c".to_owned() => Some("72 45".to_owned()),
                    "d".to_owned() => Some("gh;764".to_owned()),
                }),
                prefix: None,
                command: "FOO".to_owned(),
                params: vec![],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_pure_irc_8() {
        let source = "@c;h=;a=b :quux ab cd";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "c".to_owned() => None,
                    "h".to_owned() => Some("".to_owned()),
                    "a".to_owned() => Some("b".to_owned()),
                }),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "quux".to_owned()
                }),
                command: "AB".to_owned(),
                params: vec!["cd".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_join_1() {
        let source = ":src JOIN #chan";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "src".to_owned()
                }),
                command: "JOIN".to_owned(),
                params: vec!["#chan".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_join_2() {
        assert_eq!(
            IRCMessage::parse(":src JOIN #chan".to_owned()),
            IRCMessage::parse(":src JOIN :#chan".to_owned()),
        )
    }

    #[test]
    fn test_away_1() {
        let source = ":src AWAY";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "src".to_owned()
                }),
                command: "AWAY".to_owned(),
                params: vec![],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_away_2() {
        let source = ":cool\tguy foo bar baz";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "cool\tguy".to_owned()
                }),
                command: "FOO".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_complex_prefix() {
        let source = ":coolguy!~ag@n\u{0002}et\u{0003}05w\u{000f}ork.admin PRIVMSG foo :bar baz";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::Full {
                    nick: "coolguy".to_owned(),
                    user: Some("~ag".to_owned()),
                    host: Some("n\u{0002}et\u{0003}05w\u{000f}ork.admin".to_owned())
                }),
                command: "PRIVMSG".to_owned(),
                params: vec!["foo".to_owned(), "bar baz".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_vendor_tags() {
        let source = "@tag1=value1;tag2;vendor1/tag3=value2;vendor2/tag4 :irc.example.com COMMAND param1 param2 :param3 param3";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "tag1".to_owned() => Some("value1".to_owned()),
                    "tag2".to_owned() => None,
                    "vendor1/tag3".to_owned() => Some("value2".to_owned()),
                    "vendor2/tag4".to_owned() => None
                }),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "irc.example.com".to_owned()
                }),
                command: "COMMAND".to_owned(),
                params: vec![
                    "param1".to_owned(),
                    "param2".to_owned(),
                    "param3 param3".to_owned()
                ],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_asian_characters_display_name() {
        let source = "@display-name=테스트계정420 :tmi.twitch.tv PRIVMSG #pajlada :test";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "display-name".to_owned() => Some("테스트계정420".to_owned()),
                }),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "tmi.twitch.tv".to_owned()
                }),
                command: "PRIVMSG".to_owned(),
                params: vec!["#pajlada".to_owned(), "test".to_owned(),],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_ping_1() {
        let source = "PING :tmi.twitch.tv";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "PING".to_owned(),
                params: vec!["tmi.twitch.tv".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_ping_2() {
        let source = ":tmi.twitch.tv PING";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "tmi.twitch.tv".to_owned()
                }),
                command: "PING".to_owned(),
                params: vec![],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_invalid_empty_tags() {
        let result = IRCMessage::parse("@ :tmi.twitch.tv TEST".to_owned());
        assert_eq!(result, Err(IRCParseError::EmptyTagsDeclaration()))
    }

    #[test]
    fn test_invalid_nothing_after_tags() {
        let result = IRCMessage::parse("@key=value".to_owned());
        assert_eq!(result, Err(IRCParseError::NoSpaceAfterTags()))
    }

    #[test]
    fn test_invalid_empty_prefix() {
        let result = IRCMessage::parse("@key=value : TEST".to_owned());
        assert_eq!(result, Err(IRCParseError::EmptyPrefixDeclaration()))
    }

    #[test]
    fn test_invalid_nothing_after_prefix() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv".to_owned());
        assert_eq!(result, Err(IRCParseError::NoSpaceAfterPrefix()))
    }

    #[test]
    fn test_invalid_spaces_at_start_of_line() {
        let result = IRCMessage::parse(" @key=value :tmi.twitch.tv PING".to_owned());
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_empty_command_1() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv ".to_owned());
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_empty_command_2() {
        let result = IRCMessage::parse("".to_owned());
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_command_1() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv  PING".to_owned());
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_command_2() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv P!NG".to_owned());
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_command_3() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PØNG".to_owned());
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_command_4() {
        // mix of ascii numeric and ascii alphabetic
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv P1NG".to_owned());
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_middle_params_space_after_command() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING ".to_owned());
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams()))
    }

    #[test]
    fn test_invalid_middle_params_too_many_spaces_between_params() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING asd  def".to_owned());
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams()))
    }

    #[test]
    fn test_invalid_middle_params_too_many_spaces_after_command() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING  asd def".to_owned());
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams()))
    }

    #[test]
    fn test_invalid_middle_params_trailing_space() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING asd def ".to_owned());
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams()))
    }

    #[test]
    fn test_empty_trailing_param_1() {
        let source = "PING asd def :";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "PING".to_owned(),
                params: vec!["asd".to_owned(), "def".to_owned(), "".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_empty_trailing_param_2() {
        let source = "PING :";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "PING".to_owned(),
                params: vec!["".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
    }

    #[test]
    fn test_numeric_command() {
        let source = "500 :Internal Server Error";
        let message = IRCMessage::parse(source.to_owned()).unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "500".to_owned(),
                params: vec!["Internal Server Error".to_owned()],
                source: Some(source.to_owned())
            }
        );
        assert_eq!(IRCMessage::parse(message.as_raw_irc()).unwrap(), message);
        assert_eq!(message.as_raw_irc(), source);
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
            IRCMessage::parse("abc\ndef".to_owned()),
            Err(IRCParseError::NewlinesInMessage())
        );
        assert_eq!(
            IRCMessage::parse("abc\rdef".to_owned()),
            Err(IRCParseError::NewlinesInMessage())
        );
        assert_eq!(
            IRCMessage::parse("abc\n\rdef".to_owned()),
            Err(IRCParseError::NewlinesInMessage())
        );
    }

    #[test]
    fn test_lowercase_command() {
        assert_eq!(
            IRCMessage::parse("ping".to_owned()).unwrap().command,
            "PING"
        )
    }

    #[test]
    fn test_irc_macro() {
        assert_eq!(
            irc!["PRIVMSG"],
            IRCMessage {
                tags: IRCTags::new(),
                prefix: None,
                command: "PRIVMSG".to_owned(),
                params: vec![],
                source: None
            }
        );
        assert_eq!(
            irc!["PRIVMSG", "#pajlada"],
            IRCMessage {
                tags: IRCTags::new(),
                prefix: None,
                command: "PRIVMSG".to_owned(),
                params: vec!["#pajlada".to_owned()],
                source: None
            }
        );
        assert_eq!(
            irc!["PRIVMSG", "#pajlada", "LUL xD"],
            IRCMessage {
                tags: IRCTags::new(),
                prefix: None,
                command: "PRIVMSG".to_owned(),
                params: vec!["#pajlada".to_owned(), "LUL xD".to_owned()],
                source: None
            }
        );
    }
}
