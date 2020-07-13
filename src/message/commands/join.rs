use crate::message::commands::{IRCMessageParseExt, ServerMessageParseError};
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct JoinMessage {
    pub channel_login: String,
    pub user_login: String,
    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for JoinMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<JoinMessage, ServerMessageParseError> {
        if source.command != "JOIN" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(JoinMessage {
            channel_login: source.try_get_channel_login()?.to_owned(),
            user_login: source.try_get_prefix_nickname()?.to_owned(),
            source,
        })
    }
}

impl From<JoinMessage> for IRCMessage {
    fn from(msg: JoinMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::{IRCMessage, JoinMessage};
    use std::convert::TryFrom;

    #[test]
    pub fn test_basic() {
        let src = ":randers811!randers811@randers811.tmi.twitch.tv JOIN #pajlada";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = JoinMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            JoinMessage {
                channel_login: "pajlada".to_owned(),
                user_login: "randers811".to_owned(),
                source: irc_message
            }
        )
    }
}
