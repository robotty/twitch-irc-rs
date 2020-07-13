use crate::message::commands::{IRCMessageParseExt, ServerMessageParseError};
use crate::message::IRCMessage;
use derivative::Derivative;
use std::convert::TryFrom;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct PartMessage {
    pub channel_login: String,
    pub user_login: String,
    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

impl TryFrom<IRCMessage> for PartMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<PartMessage, ServerMessageParseError> {
        if source.command != "PART" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        Ok(PartMessage {
            channel_login: source.try_get_channel_login()?.to_owned(),
            user_login: source.try_get_prefix_nickname()?.to_owned(),
            source,
        })
    }
}

impl From<PartMessage> for IRCMessage {
    fn from(msg: PartMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::{IRCMessage, PartMessage};
    use std::convert::TryFrom;

    #[test]
    pub fn test_basic() {
        let src = ":randers811!randers811@randers811.tmi.twitch.tv PART #pajlada";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = PartMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            PartMessage {
                channel_login: "pajlada".to_owned(),
                user_login: "randers811".to_owned(),
                source: irc_message
            }
        )
    }
}
