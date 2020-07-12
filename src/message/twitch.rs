//! Twitch-specifica that only appear on Twitch-specific messages/tags.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub struct TwitchUserBasics {
    pub id: String,
    pub login: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RGBColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Emote {
    pub id: String,
    pub char_range: Range<usize>,
    pub code: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Badge {
    pub name: String,
    pub version: String,
}
