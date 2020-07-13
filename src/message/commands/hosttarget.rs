use crate::message::commands::IRCMessageParseExt;
use crate::message::{IRCMessage, ServerMessageParseError};
use derivative::Derivative;
use itertools::Itertools;
use std::convert::TryFrom;
use std::str::FromStr;

#[readonly::make]
#[derive(Debug, Clone, Derivative)]
#[derivative(PartialEq)]
pub struct HostTargetMessage {
    pub channel_login: String,
    pub action: HostTargetAction,

    #[derivative(PartialEq = "ignore")]
    pub source: IRCMessage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HostTargetAction {
    HostModeOn {
        hosted_channel_login: String,
        /// Optional: number of viewers watching the host.
        viewer_count: Option<u64>,
    },
    HostModeOff {
        /// Optional: number of viewers watching the host.
        viewer_count: Option<u64>,
    },
}

impl TryFrom<IRCMessage> for HostTargetMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<HostTargetMessage, ServerMessageParseError> {
        if source.command != "HOSTTARGET" {
            return Err(ServerMessageParseError::MismatchedCommand());
        }

        // examples:
        // host on: :tmi.twitch.tv HOSTTARGET #randers :leebaxd 0
        // host on: :tmi.twitch.tv HOSTTARGET #randers :leebaxd -
        // host off: :tmi.twitch.tv HOSTTARGET #randers :- 0

        // hosttarget_parameter is that glued-together parameter at the end, e.g. "leebaxd 0".
        // we then split it.
        let hosttarget_parameter = source.try_get_param(1)?;
        let (hosted_channel_raw, viewer_count_raw) = hosttarget_parameter
            .splitn(2, ' ')
            .next_tuple()
            .ok_or_else(|| ServerMessageParseError::MalformedParameter(1))?;

        let viewer_count = match viewer_count_raw {
            "-" => None,
            viewer_count => Some(
                u64::from_str(viewer_count)
                    .map_err(|_| ServerMessageParseError::MalformedParameter(2))?,
            ),
        };

        let action = match hosted_channel_raw {
            "-" => HostTargetAction::HostModeOff { viewer_count },
            hosted_channel_login => HostTargetAction::HostModeOn {
                hosted_channel_login: hosted_channel_login.to_owned(),
                viewer_count,
            },
        };

        Ok(HostTargetMessage {
            channel_login: source.try_get_channel_login()?.to_owned(),
            action,
            source,
        })
    }
}

impl From<HostTargetMessage> for IRCMessage {
    fn from(msg: HostTargetMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::commands::hosttarget::HostTargetAction;
    use crate::message::{HostTargetMessage, IRCMessage};
    use std::convert::TryFrom;

    #[test]
    fn test_fresh_host_on() {
        let src = ":tmi.twitch.tv HOSTTARGET #randers :leebaxd 0";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = HostTargetMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            HostTargetMessage {
                channel_login: "randers".to_owned(),
                action: HostTargetAction::HostModeOn {
                    hosted_channel_login: "leebaxd".to_owned(),
                    viewer_count: Some(0)
                },
                source: irc_message
            }
        );
    }

    #[test]
    fn test_stale_host_on() {
        let src = ":tmi.twitch.tv HOSTTARGET #randers :leebaxd -";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = HostTargetMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            HostTargetMessage {
                channel_login: "randers".to_owned(),
                action: HostTargetAction::HostModeOn {
                    hosted_channel_login: "leebaxd".to_owned(),
                    viewer_count: None
                },
                source: irc_message
            }
        );
    }

    #[test]
    fn test_host_off() {
        let src = ":tmi.twitch.tv HOSTTARGET #randers :- 0";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = HostTargetMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            HostTargetMessage {
                channel_login: "randers".to_owned(),
                action: HostTargetAction::HostModeOff {
                    viewer_count: Some(0)
                },
                source: irc_message
            }
        );
    }

    #[test]
    fn test_host_off_no_viewer_count() {
        let src = ":tmi.twitch.tv HOSTTARGET #randers :- -";
        let irc_message = IRCMessage::parse(src.to_owned()).unwrap();
        let msg = HostTargetMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            HostTargetMessage {
                channel_login: "randers".to_owned(),
                action: HostTargetAction::HostModeOff { viewer_count: None },
                source: irc_message
            }
        );
    }
}
