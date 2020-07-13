use crate::message::commands::IRCMessageParseExt;
use crate::message::{IRCMessage, ServerMessageParseError};
use chrono::{DateTime, Utc};
use derivative::Derivative;
use std::convert::TryFrom;
use std::str::FromStr;
use std::time::Duration;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct ClearChatMessage {
    channel_login: String,
    channel_id: String,
    action: ClearChatAction,
    server_timestamp: DateTime<Utc>,

    #[derivative(PartialEq = "ignore")]
    source: IRCMessage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClearChatAction {
    ChatCleared,
    UserBanned {
        user_login: String,
        user_id: String,
    },
    UserTimedOut {
        user_login: String,
        user_id: String,
        timeout_length: Duration,
    },
}

impl TryFrom<IRCMessage> for ClearChatMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<ClearChatMessage, ServerMessageParseError> {
        if source.command != "CLEARCHAT" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        // timeout example:
        // @ban-duration=1;room-id=11148817;target-user-id=148973258;tmi-sent-ts=1594553828245 :tmi.twitch.tv CLEARCHAT #pajlada :fabzeef
        // ban example:
        // @room-id=11148817;target-user-id=70948394;tmi-sent-ts=1594561360331 :tmi.twitch.tv CLEARCHAT #pajlada :weeb123
        // chat clear example:
        // @room-id=40286300;tmi-sent-ts=1594561392337 :tmi.twitch.tv CLEARCHAT #randers

        let action = match source.params.get(1) {
            Some(user_login) => {
                // ban or timeout
                let user_id = source.try_get_nonempty_tag_value("target-user-id")?;

                let ban_duration = source.try_get_optional_nonempty_tag_value("ban-duration")?;
                match ban_duration {
                    Some(ban_duration) => {
                        let ban_duration = u64::from_str(ban_duration).map_err(|_| {
                            ServerMessageParseError::MalformedTagValue(
                                "ban-duration",
                                ban_duration.to_owned(),
                            )
                        })?;

                        ClearChatAction::UserTimedOut {
                            user_login: user_login.to_owned(),
                            user_id: user_id.to_owned(),
                            timeout_length: Duration::from_secs(ban_duration),
                        }
                    }
                    None => ClearChatAction::UserBanned {
                        user_login: user_login.to_owned(),
                        user_id: user_id.to_owned(),
                    },
                }
            }
            None => ClearChatAction::ChatCleared,
        };

        Ok(ClearChatMessage {
            channel_login: source.try_get_channel_login()?.to_owned(),
            channel_id: source.try_get_nonempty_tag_value("room-id")?.to_owned(),
            action,
            server_timestamp: source.try_get_timestamp("tmi-sent-ts")?,
            source,
        })
    }
}

impl From<ClearChatMessage> for IRCMessage {
    fn from(msg: ClearChatMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::commands::clearchat::ClearChatAction;
    use crate::message::{ClearChatMessage, IRCMessage};
    use chrono::{TimeZone, Utc};
    use std::convert::TryFrom;
    use std::time::Duration;

    #[test]
    pub fn test_timeout() {
        let src = "@ban-duration=1;room-id=11148817;target-user-id=148973258;tmi-sent-ts=1594553828245 :tmi.twitch.tv CLEARCHAT #pajlada :fabzeef";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = ClearChatMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            ClearChatMessage {
                channel_login: "pajlada".to_owned(),
                channel_id: "11148817".to_owned(),
                action: ClearChatAction::UserTimedOut {
                    user_login: "fabzeef".to_owned(),
                    user_id: "148973258".to_owned(),
                    timeout_length: Duration::from_secs(1)
                },
                server_timestamp: Utc.timestamp_millis(1594553828245),
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_permaban() {
        let src = "@room-id=11148817;target-user-id=70948394;tmi-sent-ts=1594561360331 :tmi.twitch.tv CLEARCHAT #pajlada :weeb123";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = ClearChatMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            ClearChatMessage {
                channel_login: "pajlada".to_owned(),
                channel_id: "11148817".to_owned(),
                action: ClearChatAction::UserBanned {
                    user_login: "weeb123".to_owned(),
                    user_id: "70948394".to_owned(),
                },
                server_timestamp: Utc.timestamp_millis(1594561360331),
                source: irc_message
            }
        )
    }

    #[test]
    pub fn test_chat_clear() {
        let src = "@room-id=40286300;tmi-sent-ts=1594561392337 :tmi.twitch.tv CLEARCHAT #randers";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = ClearChatMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            ClearChatMessage {
                channel_login: "randers".to_owned(),
                channel_id: "40286300".to_owned(),
                action: ClearChatAction::ChatCleared,
                server_timestamp: Utc.timestamp_millis(1594561392337),
                source: irc_message
            }
        )
    }
}
