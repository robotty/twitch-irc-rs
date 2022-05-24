//! Twitch-specifica that only appear on Twitch-specific messages/tags.

use std::ops::Range;

#[cfg(feature = "with-serde")]
use {serde::Deserialize, serde::Serialize};

/// Set of information describing the basic details of a Twitch user.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct TwitchUserBasics {
    /// The user's unique ID, e.g. `103973901`
    pub id: String,
    /// The user's login name. For many users, this is simply the lowercased version of their
    /// (display) name, but there are also many users where there is no direct relation between
    /// `login` and `name`.
    ///
    /// A Twitch user can change their `login` and `name` while still keeping their `id` constant.
    /// For this reason, you should always prefer to use the `id` to uniquely identify a user, while
    /// `login` and `name` are variable properties for them.
    ///
    /// The `login` name is used in many places to refer to users, e.g. in the URL for their channel page,
    /// or also in almost all places on the Twitch IRC interface (e.g. when sending a message to a
    /// channel, you specify the channel by its login name instead of ID).
    pub login: String,
    /// Display name of the user. When possible a user should be referred to using this name
    /// in user-facing contexts.
    ///
    /// This value is never used to uniquely identify a user, and you
    /// should avoid making assumptions about the format of this value.
    /// For example, the `name` can contain non-ascii characters, it can contain spaces and
    /// it can have spaces at the start and end (albeit rare).
    pub name: String,
}

/// An RGB color, used to color chat user's names.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct RGBColor {
    /// Red component
    pub r: u8,
    /// Green component
    pub g: u8,
    /// Blue component
    pub b: u8,
}

/// A single emote, appearing as part of a message.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct Emote {
    /// An ID identifying this emote. For example `25` for the "Kappa" emote, but can also be non-numeric,
    /// for example on emotes modified using Twitch channel points, e.g.
    /// `301512758_TK` for `pajaDent_TK` where `301512758` is the ID of the original `pajaDent` emote.
    pub id: String,
    /// A range of characters in the original message where the emote is placed.
    ///
    /// As is documented on `Range`, the `start` index of this range is inclusive, while the
    /// `end` index is exclusive.
    ///
    /// This is always the exact range of characters that Twitch originally sent.
    /// Note that due to [a Twitch bug](https://github.com/twitchdev/issues/issues/104)
    /// (that this library intentionally works around), the character range specified here
    /// might be out-of-bounds for the original message text string.
    pub char_range: Range<usize>,
    /// This is the text that this emote replaces, e.g. `Kappa` or `:)`.
    pub code: String,
}

/// A single Twitch "badge" to be shown next to the user's name in chat.
///
/// The combination of `name` and `version` fully describes the exact badge to display.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "with-serde", derive(Serialize, Deserialize))]
pub struct Badge {
    /// A string identifying the type of badge. For example, `admin`, `moderator` or `subscriber`.
    pub name: String,
    /// A (usually) numeric version of this badge. Most badges only have one version (then usually
    /// version will be `0` or `1`), but other types of badges have different versions (e.g. `subscriber`)
    /// to differentiate between levels, or lengths, or similar, depending on the badge.
    pub version: String,
}

/// Extract the `msg-id` from a [`PrivmsgMessage`] or [`UsernoticeMessage`], or directly
/// use an arbitrary [`String`] or [`&str`] as a message ID. This trait allows you to plug all
/// of these types directly into
/// [`delete_message()`](crate::TwitchIRCClient::delete_message) for your convenience.
pub trait DeleteMessage {
    /// Login name of the channel that the message was sent to.
    fn channel_login(&self) -> &str;
    /// The unique string identifying the message, specified on the message via an IRCv3 tag.
    fn message_id(&self) -> &str;
}

impl<C, M> DeleteMessage for (C, M)
where
    C: AsRef<str>,
    M: AsRef<str>,
{
    fn channel_login(&self) -> &str {
        self.1.as_ref()
    }

    fn message_id(&self) -> &str {
        self.0.as_ref()
    }
}
