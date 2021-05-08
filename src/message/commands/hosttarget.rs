use crate::message::commands::IRCMessageParseExt;
use crate::message::{IRCMessage, ServerMessageParseError};
use itertools::Itertools;
use std::convert::TryFrom;
use std::str::FromStr;

#[cfg(feature = "serde-commands-support")]
use {serde::Deserialize, serde::Serialize};

/// When a channel starts or stops hosting another channel.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-commands-support", derive(Serialize, Deserialize))]
pub struct HostTargetMessage {
    /// Login name of the channel that just started or ended host mode.
    pub channel_login: String,
    /// The type of action that was taken in the channel, either host mode was enabled (entered)
    /// or disabled (exited).
    pub action: HostTargetAction,

    /// The message that this `HostTargetMessage` was parsed from.
    pub source: IRCMessage,
}

/// The type of action that a `HOSTTARGET` signifies, either host mode was enabled (entered)
/// or disabled (exited).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-commands-support", derive(Serialize, Deserialize))]
pub enum HostTargetAction {
    /// Host mode was enabled (entered).
    HostModeOn {
        /// Login name of the channel that is now being hosted.
        hosted_channel_login: String,
        /// Optional: number of viewers watching the host. If missing this number is
        /// unknown at this moment.
        viewer_count: Option<u64>,
    },
    /// Host mode was disabled (exited).
    HostModeOff {
        /// Optional: number of viewers watching the host. If missing this number is
        /// unknown at this moment.
        viewer_count: Option<u64>,
    },
}

impl TryFrom<IRCMessage> for HostTargetMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<HostTargetMessage, ServerMessageParseError> {
        if source.command != "HOSTTARGET" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
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
            .ok_or_else(|| ServerMessageParseError::MalformedParameter(source.to_owned(), 1))?;

        let viewer_count =
            match viewer_count_raw {
                "-" => None,
                viewer_count => Some(u64::from_str(viewer_count).map_err(|_| {
                    ServerMessageParseError::MalformedParameter(source.to_owned(), 2)
                })?),
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
        let irc_message = IRCMessage::parse(src).unwrap();
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
