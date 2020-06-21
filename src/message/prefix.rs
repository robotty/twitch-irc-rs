use super::AsRawIRC;
use std::fmt;

#[derive(Debug, PartialEq, Clone, Hash)]
pub enum IRCPrefix {
    HostOnly {
        host: String,
    },
    Full {
        nick: String,
        user: Option<String>,
        host: Option<String>,
    },
}

impl IRCPrefix {
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

    pub fn new_host_only(host: String) -> IRCPrefix {
        IRCPrefix::HostOnly { host }
    }

    pub fn new_full_nick_only(nick: String) -> IRCPrefix {
        IRCPrefix::Full {
            nick,
            user: None,
            host: None,
        }
    }

    pub fn new_full_nick_host(nick: String, host: String) -> IRCPrefix {
        IRCPrefix::Full {
            nick,
            user: None,
            host: Some(host),
        }
    }

    pub fn new_full(nick: String, user: String, host: String) -> IRCPrefix {
        IRCPrefix::Full {
            nick,
            user: Some(user),
            host: Some(host),
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
