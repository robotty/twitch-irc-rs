use super::AsRawIRC;
use std::fmt;

#[cfg(feature = "serde-commands-support")]
use {
    serde::Deserialize, serde::Serialize
};

/// A "prefix" part of an IRC message, as defined by RFC 2812:
/// ```none
/// <prefix>     ::= <servername> | <nick> [ '!' <user> ] [ '@' <host> ]
/// <servername> ::= <host>
/// <nick>       ::= <letter> { <letter> | <number> | <special> }
/// <user>       ::= <nonwhite> { <nonwhite> }
/// <host>       ::= see RFC 952 [DNS:4] for details on allowed hostnames
/// <letter>     ::= 'a' ... 'z' | 'A' ... 'Z'
/// <number>     ::= '0' ... '9'
/// <special>    ::= '-' | '[' | ']' | '\' | '`' | '^' | '{' | '}'
/// <nonwhite>   ::= <any 8bit code except SPACE (0x20), NUL (0x0), CR
///                   (0xd), and LF (0xa)>
/// ```
///
/// # Examples
///
/// ```
/// use twitch_irc::message::IRCPrefix;
/// use twitch_irc::message::AsRawIRC;
///
/// let prefix = IRCPrefix::Full {
///     nick: "a_nick".to_owned(),
///     user: Some("a_user".to_owned()),
///     host: Some("a_host.com".to_owned())
/// };
///
/// assert_eq!(prefix.as_raw_irc(), "a_nick!a_user@a_host.com");
/// ```
///
/// ```
/// use twitch_irc::message::IRCPrefix;
/// use twitch_irc::message::AsRawIRC;
///
/// let prefix = IRCPrefix::Full {
///     nick: "a_nick".to_owned(),
///     user: None,
///     host: Some("a_host.com".to_owned())
/// };
///
/// assert_eq!(prefix.as_raw_irc(), "a_nick@a_host.com");
/// ```
///
/// ```
/// use twitch_irc::message::IRCPrefix;
/// use twitch_irc::message::AsRawIRC;
///
/// let prefix = IRCPrefix::HostOnly {
///     host: "a_host.com".to_owned()
/// };
///
/// assert_eq!(prefix.as_raw_irc(), "a_host.com");
/// ```
#[derive(Debug, PartialEq, Clone, Hash)]
#[cfg_attr(feature = "serde-commands-support", derive(Serialize, Deserialize))]
pub enum IRCPrefix {
    /// The prefix specifies only a sending server/hostname.
    ///
    /// Note that the spec also allows a very similar format where only a sending nickname is
    /// specified. However that type of format plays no role on Twitch, and is practically impossible
    /// to reliably tell apart from host-only prefix messages. For this reason, a prefix without
    /// a `@` character is always assumed to be purely a host-only prefix, and not a nickname-only prefix.
    HostOnly {
        /// `host` part of the prefix
        host: String,
    },
    /// The prefix variant specifies a nickname, and optionally also a username and optionally a
    /// hostname. See above for the RFC definition.
    Full {
        /// `nick` part of the prefix
        nick: String,
        /// `user` part of the prefix
        user: Option<String>,
        /// `host` part of the prefix
        host: Option<String>,
    },
}

impl IRCPrefix {
    /// Parse the `IRCPrefix` from the given string slice. `source` should be specified without
    /// the leading `:` that precedes in full IRC messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use twitch_irc::message::IRCPrefix;
    ///
    /// let prefix = IRCPrefix::parse("a_nick!a_user@a_host.com");
    /// assert_eq!(prefix, IRCPrefix::Full {
    ///     nick: "a_nick".to_owned(),
    ///     user: Some("a_user".to_owned()),
    ///     host: Some("a_host.com".to_owned())
    /// })
    /// ```
    ///
    /// ```
    /// use twitch_irc::message::IRCPrefix;
    ///
    /// let prefix = IRCPrefix::parse("a_host.com");
    /// assert_eq!(prefix, IRCPrefix::HostOnly {
    ///     host: "a_host.com".to_owned()
    /// })
    /// ```
    pub fn parse(source: &str) -> IRCPrefix {
        if !source.contains('@') {
            // just a hostname
            IRCPrefix::HostOnly {
                host: source.to_owned(),
            }
        } else {
            // full prefix (nick[[!user]@host])
            // valid forms:
            // nick
            // nick@host
            // nick!user@host

            // split on @ first, then on !
            let mut at_split = source.splitn(2, '@');
            let nick_and_user = at_split.next().unwrap();
            let host = at_split.next();

            // now nick_and_user is either "nick" or "nick!user"
            let mut exc_split = nick_and_user.splitn(2, '!');
            let nick = exc_split.next();
            let user = exc_split.next();

            IRCPrefix::Full {
                nick: nick.unwrap().to_owned(),
                user: user.map(|s| s.to_owned()),
                host: host.map(|s| s.to_owned()),
            }
        }
    }
}

impl AsRawIRC for IRCPrefix {
    fn format_as_raw_irc(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        match self {
            Self::HostOnly { host } => write!(f, "{}", host)?,
            Self::Full { nick, user, host } => {
                write!(f, "{}", nick)?;
                if let Some(host) = host {
                    if let Some(user) = user {
                        write!(f, "!{}", user)?
                    }
                    write!(f, "@{}", host)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_host_only() {
        let prefix = IRCPrefix::HostOnly {
            host: "tmi.twitch.tv".to_owned(),
        };
        assert_eq!(prefix.as_raw_irc(), "tmi.twitch.tv");
    }

    #[test]
    fn test_display_full_1() {
        let prefix = IRCPrefix::Full {
            nick: "justin".to_owned(),
            user: Some("justin".to_owned()),
            host: Some("justin.tmi.twitch.tv".to_owned()),
        };
        assert_eq!(prefix.as_raw_irc(), "justin!justin@justin.tmi.twitch.tv");
    }

    #[test]
    fn test_display_full_2() {
        let prefix = IRCPrefix::Full {
            nick: "justin".to_owned(),
            user: None,
            host: Some("justin.tmi.twitch.tv".to_owned()),
        };
        assert_eq!(prefix.as_raw_irc(), "justin@justin.tmi.twitch.tv");
    }

    #[test]
    fn test_display_full_3() {
        let prefix = IRCPrefix::Full {
            nick: "justin".to_owned(),
            user: None,
            host: None,
        };
        assert_eq!(prefix.as_raw_irc(), "justin");
    }

    #[test]
    fn test_display_full_4_user_without_host_invalid_edge_case() {
        let prefix = IRCPrefix::Full {
            nick: "justin".to_owned(),
            user: Some("justin".to_owned()),
            host: None,
        };
        assert_eq!(prefix.as_raw_irc(), "justin");
    }
}
