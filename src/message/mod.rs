pub mod prefix;
pub mod stream;
pub mod tags;

use self::prefix::IRCPrefix;
use self::tags::IRCTags;
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

#[derive(Debug, PartialEq)]
pub struct IRCMessage {
    tags: IRCTags,
    prefix: Option<IRCPrefix>,
    command: String,
    params: Vec<String>,
}

impl IRCMessage {
    pub fn new_simple<C: Into<String>, P: Into<String>>(command: C, params: Vec<P>) -> IRCMessage {
        IRCMessage {
            tags: IRCTags::new(),
            prefix: None,
            command: command.into(),
            params: params.into_iter().map(|x| x.into()).collect_vec(),
        }
    }

    pub fn parse(mut str: &str) -> Result<IRCMessage, IRCParseError> {
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
        let command = command_split.next().unwrap();

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
            command: command.to_owned(),
            params,
        })
    }
}

impl AsRawIRC for IRCMessage {
    fn format_as_raw_irc(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.tags.get_map().is_empty() {
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
        let message = IRCMessage::parse("@rm-received-ts=1577040815136;historical=1;badge-info=subscriber/16;badges=moderator/1,subscriber/12;color=#19E6E6;display-name=randers;emotes=;flags=;id=6e2ccb1f-01ed-44d0-85b6-edf762524475;mod=1;room-id=11148817;subscriber=1;tmi-sent-ts=1577040814959;turbo=0;user-id=40286300;user-type=mod :randers!randers@randers.tmi.twitch.tv PRIVMSG #pajlada :Pajapains").unwrap();
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
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_confusing_prefix_trailing_param() {
        let message = IRCMessage::parse(":coolguy foo bar baz asdf").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "foo".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned(), "asdf".to_owned()],
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_1() {
        let message = IRCMessage::parse("foo bar baz ::asdf").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "foo".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned(), ":asdf".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_2() {
        let message = IRCMessage::parse(":coolguy foo bar baz :  asdf quux ").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "foo".to_owned(),
                params: vec![
                    "bar".to_owned(),
                    "baz".to_owned(),
                    "  asdf quux ".to_owned()
                ]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_3() {
        let message = IRCMessage::parse(":coolguy PRIVMSG bar :lol :) ").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "PRIVMSG".to_owned(),
                params: vec!["bar".to_owned(), "lol :) ".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_4() {
        let message = IRCMessage::parse(":coolguy foo bar baz :").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "foo".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned(), "".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_5() {
        let message = IRCMessage::parse(":coolguy foo bar baz :  ").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "coolguy".to_owned()
                }),
                command: "foo".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned(), "  ".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_6() {
        let message = IRCMessage::parse("@a=b;c=32;k;rt=ql7 foo").unwrap();
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
                command: "foo".to_owned(),
                params: vec![]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_7() {
        let message = IRCMessage::parse("@a=b\\\\and\\nk;c=72\\s45;d=gh\\:764 foo").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {
                    "a".to_owned() => Some("b\\and\nk".to_owned()),
                    "c".to_owned() => Some("72 45".to_owned()),
                    "d".to_owned() => Some("gh;764".to_owned()),
                }),
                prefix: None,
                command: "foo".to_owned(),
                params: vec![]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_pure_irc_8() {
        let message = IRCMessage::parse("@c;h=;a=b :quux ab cd").unwrap();
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
                command: "ab".to_owned(),
                params: vec!["cd".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_join_1() {
        let message = IRCMessage::parse(":src JOIN #chan").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "src".to_owned()
                }),
                command: "JOIN".to_owned(),
                params: vec!["#chan".to_owned()]
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
        let message = IRCMessage::parse(":src AWAY").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "src".to_owned()
                }),
                command: "AWAY".to_owned(),
                params: vec![]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_away_2() {
        let message = IRCMessage::parse(":cool\tguy foo bar baz").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "cool\tguy".to_owned()
                }),
                command: "foo".to_owned(),
                params: vec!["bar".to_owned(), "baz".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_complex_prefix() {
        let message = IRCMessage::parse(
            ":coolguy!~ag@n\u{0002}et\u{0003}05w\u{000f}ork.admin PRIVMSG foo :bar baz",
        )
        .unwrap();
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
                params: vec!["foo".to_owned(), "bar baz".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_vendor_tags() {
        let message = IRCMessage::parse("@tag1=value1;tag2;vendor1/tag3=value2;vendor2/tag4 :irc.example.com COMMAND param1 param2 :param3 param3").unwrap();
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
                ]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_asian_characters_display_name() {
        let message =
            IRCMessage::parse("@display-name=테스트계정420 :tmi.twitch.tv PRIVMSG #pajlada :test")
                .unwrap();
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
                params: vec!["#pajlada".to_owned(), "test".to_owned(),]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_ping_1() {
        let message = IRCMessage::parse("PING :tmi.twitch.tv").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "PING".to_owned(),
                params: vec!["tmi.twitch.tv".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_ping_2() {
        let message = IRCMessage::parse(":tmi.twitch.tv PING").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: Some(IRCPrefix::HostOnly {
                    host: "tmi.twitch.tv".to_owned()
                }),
                command: "PING".to_owned(),
                params: vec![]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_invalid_empty_tags() {
        let result = IRCMessage::parse("@ :tmi.twitch.tv TEST");
        assert_eq!(result, Err(IRCParseError::EmptyTagsDeclaration()))
    }

    #[test]
    fn test_invalid_nothing_after_tags() {
        let result = IRCMessage::parse("@key=value");
        assert_eq!(result, Err(IRCParseError::NoSpaceAfterTags()))
    }

    #[test]
    fn test_invalid_empty_prefix() {
        let result = IRCMessage::parse("@key=value : TEST");
        assert_eq!(result, Err(IRCParseError::EmptyPrefixDeclaration()))
    }

    #[test]
    fn test_invalid_nothing_after_prefix() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv");
        assert_eq!(result, Err(IRCParseError::NoSpaceAfterPrefix()))
    }

    #[test]
    fn test_invalid_spaces_at_start_of_line() {
        let result = IRCMessage::parse(" @key=value :tmi.twitch.tv PING");
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_empty_command_1() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv ");
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_empty_command_2() {
        let result = IRCMessage::parse("");
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_command_1() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv  PING");
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_command_2() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv P!NG");
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_command_3() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PØNG");
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_command_4() {
        // mix of ascii numeric and ascii alphabetic
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv P1NG");
        assert_eq!(result, Err(IRCParseError::MalformedCommand()))
    }

    #[test]
    fn test_invalid_middle_params_space_after_command() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING ");
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams()))
    }

    #[test]
    fn test_invalid_middle_params_too_many_spaces_between_params() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING asd  def");
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams()))
    }

    #[test]
    fn test_invalid_middle_params_too_many_spaces_after_command() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING  asd def");
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams()))
    }

    #[test]
    fn test_invalid_middle_params_trailing_space() {
        let result = IRCMessage::parse("@key=value :tmi.twitch.tv PING asd def ");
        assert_eq!(result, Err(IRCParseError::TooManySpacesInMiddleParams()))
    }

    #[test]
    fn test_empty_trailing_param_1() {
        let message = IRCMessage::parse("PING asd def :").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "PING".to_owned(),
                params: vec!["asd".to_owned(), "def".to_owned(), "".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_empty_trailing_param_2() {
        let message = IRCMessage::parse("PING :").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "PING".to_owned(),
                params: vec!["".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_numeric_command() {
        let message = IRCMessage::parse("500 :Internal Server Error").unwrap();
        assert_eq!(
            message,
            IRCMessage {
                tags: IRCTags::from(hashmap! {}),
                prefix: None,
                command: "500".to_owned(),
                params: vec!["Internal Server Error".to_owned()]
            }
        );
        assert_eq!(IRCMessage::parse(&message.as_raw_irc()).unwrap(), message);
    }

    #[test]
    fn test_stringify_pass() {
        assert_eq!(
            IRCMessage::new_simple("PASS", vec!["oauth:9892879487293847"]).as_raw_irc(),
            "PASS oauth:9892879487293847"
        );
    }
}
