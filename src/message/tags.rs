use super::AsRawIRC;
use itertools::Itertools;
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::fmt;
use std::fmt::Write;

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

fn decode_tag_value(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());

    let mut iter = raw.chars();
    while let Some(c) = iter.next() {
        if c == '\\' {
            let next_char = iter.next();
            match next_char {
                Some(':') => output.push(';'),   // \: escapes to ;
                Some('s') => output.push(' '),   // \s decodes to a space
                Some('\\') => output.push('\\'), // \\ decodes to \
                Some('r') => output.push('\r'),  // \r decodes to CR
                Some('n') => output.push('\n'),  // \n decodes to LF
                Some(c) => output.push(c),       // E.g. a\bc escapes to abc
                None => {}                       // Dangling \ at the end of the string
            }
        } else {
            // No escape sequence here
            output.push(c);
        }
    }
    output
}

fn encode_tag_value(raw: &str) -> String {
    let mut output = String::with_capacity((raw.len() as f64 * 1.2) as usize);

    for c in raw.chars() {
        match c {
            ';' => output.push_str("\\:"),
            ' ' => output.push_str("\\s"),
            '\\' => output.push_str("\\\\"),
            '\r' => output.push_str("\\r"),
            '\n' => output.push_str("\\n"),
            c => output.push(c),
        };
    }

    output
}

/// A map of key-value [IRCv3 tags](https://ircv3.net/specs/extensions/message-tags.html).
///
/// # Examples
///
/// ```
/// use twitch_irc::message::IRCTags;
/// use twitch_irc::message::AsRawIRC;
/// use maplit::hashmap;
///
/// let tags = IRCTags::parse("key=value;key2=value2;key3");
/// assert_eq!(tags, hashmap! {
///     "key".to_owned() => Some("value".to_owned()),
///     "key2".to_owned() => Some("value2".to_owned()),
///     "key3".to_owned() => None
/// })
/// ```
#[derive(Debug, PartialEq, Eq, Clone, Default)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct IRCTags(pub HashMap<String, Option<String>>);

impl IRCTags {
    /// Creates a new empty map of tags.
    pub fn new() -> IRCTags {
        IRCTags(HashMap::new())
    }

    /// Parses a new set of tags from their wire-format representation.
    /// `source` should be specified without the leading `@` present in the full IRC tags.
    ///
    /// # Panics
    /// Panics if `source` is an empty string.
    pub fn parse(source: &str) -> IRCTags {
        if source.is_empty() {
            panic!("invalid input")
        }

        let mut tags = IRCTags::new();

        for raw_tag in source.split(';') {
            let mut tag_split = raw_tag.splitn(2, '=');

            // always expected to be present, even splitting an empty string yields [""]
            let key = tag_split.next().unwrap();
            // can be missing if no = is present
            let value = tag_split.next().map(decode_tag_value);

            tags.0.insert(key.to_owned(), value);
        }

        tags
    }
}

impl From<HashMap<String, Option<String>>> for IRCTags {
    fn from(map: HashMap<String, Option<String>, RandomState>) -> Self {
        IRCTags(map)
    }
}

impl AsRawIRC for IRCTags {
    fn format_as_raw_irc(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut add_separator = false;
        for (key, value) in self.0.iter().sorted() {
            if add_separator {
                f.write_char(';')?;
            } else {
                add_separator = true;
            }
            f.write_str(key)?;
            if let Some(value) = value {
                f.write_char('=')?;
                f.write_str(&encode_tag_value(value))?;
            }
        }

        Ok(())
    }
}

impl PartialEq<HashMap<String, Option<String>>> for IRCTags {
    fn eq(&self, other: &HashMap<String, Option<String>, RandomState>) -> bool {
        &self.0 == other
    }
}

impl PartialEq<IRCTags> for HashMap<String, Option<String>> {
    fn eq(&self, other: &IRCTags) -> bool {
        self == &other.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashmap;

    #[test]
    fn test_parse_tag_no_value() {
        let tags = IRCTags::parse("key=value;asd;def=");

        assert_eq!(
            tags,
            hashmap! {
                "key".to_owned() => Some("value".to_owned()),
                "asd".to_owned() => None,
                "def".to_owned() => Some("".to_owned()),
            }
        );
    }

    #[test]
    fn test_override_1() {
        let tags = IRCTags::parse("key=value;key=;key");

        assert_eq!(
            tags,
            hashmap! {
                "key".to_owned() => None
            }
        );
    }

    #[test]
    fn test_override_2() {
        let tags = IRCTags::parse("key;key=;key=value");

        assert_eq!(
            tags,
            hashmap! {
                "key".to_owned() => Some("value".to_owned())
            }
        );
    }

    #[test]
    fn test_decode_unescape_1() {
        let tags = IRCTags::parse("key=The\\sLazy\\sDog");

        assert_eq!(
            tags,
            hashmap! {
                "key".to_owned() => Some("The Lazy Dog".to_owned())
            }
        );
    }

    #[test]
    fn test_decode_unescape_dangling_backslash_at_end() {
        let tags = IRCTags::parse("key=The\\sLazy\\sDog\\");

        assert_eq!(
            tags,
            hashmap! {
                "key".to_owned() => Some("The Lazy Dog".to_owned())
            }
        );
    }

    #[test]
    fn test_decode_unescapes_dangling_backslash() {
        let tags = IRCTags::parse("key=\\The\\sLazy\\sDog");

        assert_eq!(
            tags,
            hashmap! {
                "key".to_owned() => Some("The Lazy Dog".to_owned())
            }
        );
    }

    #[test]
    fn test_decode_unescapes_all_decode_sequences() {
        assert_eq!(
            IRCTags::parse("key=\\:"),
            hashmap! {
                "key".to_owned() => Some(";".to_owned())
            }
        );
        assert_eq!(
            IRCTags::parse("key=\\s"),
            hashmap! {
                "key".to_owned() => Some(" ".to_owned())
            }
        );
        assert_eq!(
            IRCTags::parse("key=\\\\"),
            hashmap! {
                "key".to_owned() => Some("\\".to_owned())
            }
        );
        assert_eq!(
            IRCTags::parse("key=\\r"),
            hashmap! {
                "key".to_owned() => Some("\r".to_owned())
            }
        );
        assert_eq!(
            IRCTags::parse("key=\\n"),
            hashmap! {
                "key".to_owned() => Some("\n".to_owned())
            }
        );
        assert_eq!(
            IRCTags::parse("key=\\:\\s\\\\\\r\\n"),
            hashmap! {
                "key".to_owned() => Some("; \\\r\n".to_owned())
            }
        );
    }
}
