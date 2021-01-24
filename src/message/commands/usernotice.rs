use crate::message::commands::IRCMessageParseExt;
use crate::message::twitch::{Badge, Emote, RGBColor, TwitchUserBasics};
use crate::message::{IRCMessage, ServerMessageParseError};
use chrono::{DateTime, Utc};
use std::convert::TryFrom;

/// A Twitch `USERNOTICE` message.
///
/// The `USERNOTICE` message represents a wide variety of "rich events" in chat,
/// e.g. sub events, resubs, gifted subscriptions, incoming raids, etc.
///
/// See `UserNoticeEvent` for more details on all the different events.
#[derive(Debug, Clone, PartialEq)]
pub struct UserNoticeMessage {
    /// Login name of the channel that this message was sent to.
    pub channel_login: String,
    /// ID of the channel that this message was sent to.
    pub channel_id: String,

    /// The user that sent/triggered this message. Depending on the `event` (see below),
    /// this user may or may not have any actual meaning (for some type of events, this
    /// user is a dummy user).
    ///
    /// Even if this user is not a dummy user, the meaning of what this user did depends on the
    /// `event` that this `USERNOTICE` message represents. For example, in case of a raid,
    /// this user is the user raiding the channel, in case of a `sub`, it's the user
    /// subscribing, etc...)
    pub sender: TwitchUserBasics,

    /// If present, an optional message the user sent alongside the notification. Not all types
    /// of events can have message text.
    ///
    /// Currently the only event that can a message is a `resub`, where this message text is the
    /// message the user shared with the streamer alongside the resub message.
    pub message_text: Option<String>,
    /// A system message that is always present and represents a user-presentable message
    /// of what this event is, for example "FuchsGewand subscribed with Twitch Prime.
    /// They've subscribed for 12 months, currently on a 9 month streak!".
    ///
    /// This message is always present and always fully pre-formatted by Twitch
    /// with this event's parameters.
    pub system_message: String,

    /// this holds the event-specific data, e.g. for sub, resub, subgift, etc...
    pub event: UserNoticeEvent,

    /// String identifying the type of event (`msg-id` tag). Can be used to manually parse
    /// undocumented types of `USERNOTICE` messages.
    pub event_id: String,

    /// Metadata related to the chat badges in the `badges` tag.
    ///
    /// Currently this is used only for `subscriber`, to indicate the exact number of months
    /// the user has been a subscriber. This number is finer grained than the version number in
    /// badges. For example, a user who has been a subscriber for 45 months would have a
    /// `badge_info` value of 45 but might have a `badges` `version` number for only 3 years.
    pub badge_info: Vec<Badge>,
    /// List of badges that should be displayed alongside the message.
    pub badges: Vec<Badge>,
    /// A list of emotes in this message. Each emote replaces a part of the `message_text`.
    /// These emotes are sorted in the order that they appear in the message.
    ///
    /// If `message_text` is `None`, this is an empty list and carries no information (since
    /// there is no message, and therefore no emotes to display)
    pub emotes: Vec<Emote>,

    /// If present, specifies the color that the user's name should be displayed in. A value
    /// of `None` here signifies that the user has not picked any particular color.
    /// Implementations differ on how they handle this, on the Twitch website users are assigned
    /// a pseudorandom but consistent-per-user color if they have no color specified.
    pub name_color: Option<RGBColor>,

    /// A string uniquely identifying this message. Can be used with `/delete <message_id>` to
    /// delete single messages (see also the `CLEARMSG` message type)
    pub message_id: String,

    /// Timestamp of when this message was sent.
    pub server_timestamp: DateTime<Utc>,

    /// The message that this `UserNoticeMessage` was parsed from.
    pub source: IRCMessage,
}

/// Additionally present on `giftpaidupgrade` and `anongiftpaidupgrade` messages
/// if the upgrade happens as part of a seasonal promotion on Twitch, e.g. Subtember
/// or similar.
#[derive(Debug, Clone, PartialEq)]
pub struct SubGiftPromo {
    /// Total number of subs gifted during this promotion
    pub total_gifts: u64,
    /// Friendly name of the promotion, e.g. `Subtember 2018`
    pub promo_name: String,
}

impl SubGiftPromo {
    fn parse_if_present(
        source: &IRCMessage,
    ) -> Result<Option<SubGiftPromo>, ServerMessageParseError> {
        if let (Some(total_gifts), Some(promo_name)) = (
            source.try_get_optional_number("msg-param-promo-gift-total")?,
            source
                .try_get_optional_nonempty_tag_value("msg-param-promo-name")?
                .map(|s| s.to_owned()),
        ) {
            Ok(Some(SubGiftPromo {
                total_gifts,
                promo_name,
            }))
        } else {
            Ok(None)
        }
    }
}

/// A type of event that a `UserNoticeMessage` represents.
///
/// The `USERNOTICE` command is used for a wide variety of different "rich events" on
/// the Twitch platform. This enum provides parsed variants for a variety of documented
/// type of events.
///
/// However Twitch has been known to often add new events without prior notice or even
/// documenting them. For this reason, one should never expect this list to be exhaustive.
/// All events that don't have a more concrete representation inside this enum get parsed
/// as a `UserNoticeEvent::Unknown` (which is hidden from the documentation on purpose):
/// You should always use the `_` rest-branch and `event_id` when manually parsing other events.
///
/// ```rust
/// # use twitch_irc::message::{UserNoticeMessage, UserNoticeEvent, IRCMessage};
/// # use std::convert::TryFrom;
/// let message = UserNoticeMessage::try_from(IRCMessage::parse("@badge-info=subscriber/2;badges=subscriber/2,bits/1000;color=#FF4500;display-name=whoopiix;emotes=;flags=;id=d2b32a02-3071-4c52-b2ce-bc3716acdc44;login=whoopiix;mod=0;msg-id=bitsbadgetier;msg-param-threshold=1000;room-id=71092938;subscriber=1;system-msg=bits\\sbadge\\stier\\snotification;tmi-sent-ts=1594520403813;user-id=104252055;user-type= :tmi.twitch.tv USERNOTICE #xqcow").unwrap()).unwrap();
/// match &message.event {
///     UserNoticeEvent::BitsBadgeTier { threshold } => println!("{} just unlocked the {} bits badge!", message.sender.name, threshold),
///     _ => println!("some other type of event: {}", message.event_id)
/// }
/// ```
///
/// This enum is also marked as `#[non_exhaustive]` to signify that more events may be
/// added to it in the future, without the need for a breaking release.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum UserNoticeEvent {
    /// Emitted when a user subscribes or resubscribes to a channel.
    /// The user sending this `USERNOTICE` is the user subscribing/resubscribing.
    ///
    /// For brevity this event captures both `sub` and `resub` events because they both
    /// carry the exact same parameters. You can differentiate between the two events using
    /// `is_resub`, which is false for `sub` and true for `resub` events.
    SubOrResub {
        /// Indicates whether this is a first-time sub or a resub.
        is_resub: bool,
        /// Cumulative number of months the sending user has subscribed to this channel.
        cumulative_months: u64,
        /// Consecutive number of months the sending user has subscribed to this channel.
        streak_months: Option<u64>,
        /// `Prime`, `1000`, `2000` or `3000`, referring to Prime or tier 1, 2 or 3 subs respectively.
        sub_plan: String,
        /// A name the broadcaster configured for this sub plan, e.g. `The Ninjas` or
        /// `Channel subscription (nymn_hs)`
        sub_plan_name: String,
    },

    /// Incoming raid to a channel.
    /// The user sending this `USERNOTICE` message is the user raiding this channel.
    Raid {
        /// How many viewers participated in the raid and just raided this channel.
        viewer_count: u64,
        /// A link to the profile image of the raiding user. This is not officially documented
        /// Empirical evidence suggests this is always the 70x70 version of the full profile
        /// picture.
        ///
        /// E.g. `https://static-cdn.jtvnw.net/jtv_user_pictures/cae3ca63-510d-4715-b4ce-059dcf938978-profile_image-70x70.png`
        profile_image_url: String,
    },

    /// Indicates a gifted subscription.
    ///
    /// This event combines `subgift` and `anonsubgift`. In case of
    /// `anonsubgift` the sending user of the `USERNOTICE` carries no useful information,
    /// it can be e.g. the channel owner or a service user like `AnAnonymousGifter`. You should
    /// always check for `is_sender_anonymous` before using the sender of the `USERNOTICE`.
    SubGift {
        /// Indicates whether the user sending this `USERNOTICE` is a dummy or a real gifter.
        /// If this is `true` the gift comes from an anonymous user, and the user sending the
        /// `USERNOTICE` carries no useful information and should be ignored.
        is_sender_anonymous: bool,
        /// Cumulative number of months the recipient has subscribed to this channel.
        cumulative_months: u64,
        /// The user that received this gifted subscription or resubscription.
        recipient: TwitchUserBasics,
        /// `1000`, `2000` or `3000`, referring to tier 1, 2 or 3 subs respectively.
        sub_plan: String,
        /// A name the broadcaster configured for this sub plan, e.g. `The Ninjas` or
        /// `Channel subscription (nymn_hs)`
        sub_plan_name: String,
        /// number of months in a single multi-month gift.
        num_gifted_months: u64,
    },

    /// This event precedes a wave of `subgift`/`anonsubgift` messages.
    /// (`<User> is gifting <mass_gift_count> Tier 1 Subs to <Channel>'s community! They've gifted a total of <sender_total_gifts> in the channel!`)
    ///
    /// This event combines `submysterygift` and `anonsubmysterygift`. In case of
    /// `anonsubmysterygift` the sending user of the `USERNOTICE` carries no useful information,
    /// it can be e.g. the channel owner or a service user like `AnAnonymousGifter`. You should
    /// always check for `is_sender_anonymous` before using the sender of the `USERNOTICE`.
    SubMysteryGift {
        /// Indicates whether the user sending this `USERNOTICE` is a dummy or a real gifter.
        /// If this is `true` the gift comes from an anonymous user, and the user sending the
        /// `USERNOTICE` carries no useful information and should be ignored.
        /// Number of gifts the sender just gifted.
        mass_gift_count: u64,
        /// Total number of gifts the sender has gifted in this channel. This includes the
        /// number of gifts in this `submysterygift` or `anonsubmysterygift`.
        /// Note tha
        sender_total_gifts: u64,
        /// The type of sub plan the recipients were gifted.
        /// `1000`, `2000` or `3000`, referring to tier 1, 2 or 3 subs respectively.
        sub_plan: String,
    },

    /// This event precedes a wave of `subgift`/`anonsubgift` messages.
    /// (`An anonymous user is gifting <mass_gift_count> Tier 1 Subs to <Channel>'s community!`)
    ///
    /// This is a variant of `submysterygift` where the sending user is not known.
    /// Not that even though every `USERNOTICE` carries a sending user, the sending user of this
    /// type of `USERNOTICE` carries no useful information, it can be e.g. the channel owner
    /// or a service user like `AnAnonymousGifter`.
    ///
    /// Compared to `submysterygift` this does not provide `sender_total_gifts`.
    AnonSubMysteryGift {
        /// Number of gifts the sender just gifted.
        mass_gift_count: u64,
        /// The type of sub plan the recipients were gifted.
        /// `1000`, `2000` or `3000`, referring to tier 1, 2 or 3 subs respectively.
        sub_plan: String,
    },

    /// Occurs when a user continues their gifted subscription they got from a non-anonymous
    /// gifter.
    ///
    /// The sending user of this `USERNOTICE` is the user upgrading their sub.
    /// The user that gifted the original gift sub is specified by these params.
    GiftPaidUpgrade {
        /// User that originally gifted the sub to this user.
        /// This is the login name, see `TwitchUserBasics` for more info about the difference
        /// between id, login and name.
        gifter_login: String,
        /// User that originally gifted the sub to this user.
        /// This is the (display) name name, see `TwitchUserBasics` for more info about the
        /// difference between id, login and name.
        gifter_name: String,
        /// Present if this gift/upgrade is part of a Twitch gift sub promotion, e.g.
        /// Subtember or similar.
        promotion: Option<SubGiftPromo>,
    },

    /// Occurs when a user continues their gifted subscription they got from an anonymous
    /// gifter.
    ///
    /// The sending user of this `USERNOTICE` is the user upgrading their sub.
    AnonGiftPaidUpgrade {
        /// Present if this gift/upgrade is part of a Twitch gift sub promotion, e.g.
        /// Subtember or similar.
        promotion: Option<SubGiftPromo>,
    },

    /// A user is new in a channel and uses the rituals feature to send a message letting
    /// the chat know they are new.
    /// `<Sender> is new to <Channel>'s chat! Say hello!`
    Ritual {
        /// currently only valid value: `new_chatter`
        ritual_name: String,
    },

    /// When a user cheers and earns himself a new bits badge with that cheer
    /// (e.g. they just cheered more than/exactly 10000 bits in total,
    /// and just earned themselves the 10k bits badge)
    BitsBadgeTier {
        /// tier of bits badge the user just earned themselves, e.g. `10000` if they just
        /// earned the 10k bits badge.
        threshold: u64,
    },

    // this is hidden so users don't match on it. Instead they should match on _
    // so their code still works the same when new variants are added here.
    #[doc(hidden)]
    Unknown,
}

impl TryFrom<IRCMessage> for UserNoticeMessage {
    type Error = ServerMessageParseError;

    fn try_from(source: IRCMessage) -> Result<UserNoticeMessage, ServerMessageParseError> {
        if source.command != "USERNOTICE" {
            return Err(ServerMessageParseError::MismatchedCommand(source));
        }

        // example message:
        // @badge-info=subscriber/6;badges=subscriber/6,sub-gifter/1;color=#FF0000;display-name=9966Qtips;emotes=;flags=;id=916cdb58-87b6-407c-a54c-f79c54248aa7;login=9966qtips;mod=0;msg-id=resub;msg-param-cumulative-months=6;msg-param-months=0;msg-param-should-share-streak=0;msg-param-sub-plan-name=Channel\sSubscription\s(xqcow);msg-param-sub-plan=Prime;room-id=71092938;subscriber=1;system-msg=9966Qtips\ssubscribed\swith\sTwitch\sPrime.\sThey've\ssubscribed\sfor\s6\smonths!;tmi-sent-ts=1575162201680;user-id=46977320;user-type= :tmi.twitch.tv USERNOTICE #xqcow :xqcJAM xqcJAM xqcJAM xqcJAM

        // note the message can also be missing:
        // also note emotes= is still present
        // @badge-info=subscriber/0;badges=subscriber/0,premium/1;color=#8A2BE2;display-name=PilotChup;emotes=;flags=;id=c7ae5c7a-3007-4f9d-9e64-35219a5c1134;login=pilotchup;mod=0;msg-id=sub;msg-param-cumulative-months=1;msg-param-months=0;msg-param-should-share-streak=0;msg-param-sub-plan-name=Channel\sSubscription\s(xqcow);msg-param-sub-plan=Prime;room-id=71092938;subscriber=1;system-msg=PilotChup\ssubscribed\swith\sTwitch\sPrime.;tmi-sent-ts=1575162111790;user-id=40745007;user-type= :tmi.twitch.tv USERNOTICE #xqcow

        let sender = TwitchUserBasics {
            id: source.try_get_nonempty_tag_value("user-id")?.to_owned(),
            login: source.try_get_nonempty_tag_value("login")?.to_owned(),
            name: source
                .try_get_nonempty_tag_value("display-name")?
                .to_owned(),
        };

        // the `msg-id` tag specifies the type of event this usernotice conveys. According to twitch,
        // the value can be one of:
        // sub, resub, raid, subgift, anonsubgift, anongiftpaidupgrade, giftpaidupgrade, ritual, bitsbadgetier
        // more types are often added by Twitch ad-hoc without prior notice as part
        // of seasonal events.
        // TODO msg-id's that have been seen but are not documented:
        //  rewardgift, primepaidupgrade, extendsub, standardpayforward, communitypayforward
        //  (these can be added later)
        // each event then has additional tags beginning with `msg-param-`, see below

        let event_id = source.try_get_nonempty_tag_value("msg-id")?.to_owned();
        let event = match event_id.as_str() {
            // sub, resub:
            // sender is the user subbing/resubbung
            // msg-param-cumulative-months
            // msg-param-should-share-streak
            // msg-param-streak-months
            // msg-param-sub-plan (1000, 2000 or 3000 for the three sub tiers, and Prime)
            // msg-param-sub-plan-name (e.g. "The Ninjas")
            "sub" | "resub" => UserNoticeEvent::SubOrResub {
                is_resub: &event_id == "resub",
                cumulative_months: source.try_get_number("msg-param-cumulative-months")?,
                streak_months: if source.try_get_bool("msg-param-should-share-streak")? {
                    Some(source.try_get_number("msg-param-streak-months")?)
                } else {
                    None
                },
                sub_plan: source
                    .try_get_nonempty_tag_value("msg-param-sub-plan")?
                    .to_owned(),
                sub_plan_name: source
                    .try_get_nonempty_tag_value("msg-param-sub-plan-name")?
                    .to_owned(),
            },
            // raid:
            // sender is the user raiding this channel
            // msg-param-displayName (duplicates always-present display-name tag)
            // msg-param-login (duplicates always-present login tag)
            // msg-param-viewerCount
            // msg-param-profileImageURL (link to 70x70 version of raider's pfp)
            "raid" => UserNoticeEvent::Raid {
                viewer_count: source.try_get_number::<u64>("msg-param-viewerCount")?,
                profile_image_url: source
                    .try_get_nonempty_tag_value("msg-param-profileImageURL")?
                    .to_owned(),
            },
            // subgift, anonsubgift:
            // sender of message is the gifter, or AnAnonymousGifter (ID 274598607)
            // msg-param-months (same as msg-param-cumulative-months on sub/resub)
            // msg-param-recipient-display-name
            // msg-param-recipient-id
            // msg-param-recipient-user-name (login name)
            // msg-param-sub-plan (1000, 2000 or 3000 for the three sub tiers)
            // msg-param-sub-plan-name (e.g. "The Ninjas")
            // msg-param-gift-months (number of months in a single multi-month gift)
            "subgift" | "anonsubgift" => UserNoticeEvent::SubGift {
                // 274598607 is the user ID of "AnAnonymousGifter"
                is_sender_anonymous: event_id == "anonsubgift" || sender.id == "274598607",
                cumulative_months: source.try_get_number("msg-param-months")?,
                recipient: TwitchUserBasics {
                    id: source
                        .try_get_nonempty_tag_value("msg-param-recipient-id")?
                        .to_owned(),
                    login: source
                        .try_get_nonempty_tag_value("msg-param-recipient-user-name")?
                        .to_owned(),
                    name: source
                        .try_get_nonempty_tag_value("msg-param-recipient-display-name")?
                        .to_owned(),
                },
                sub_plan: source
                    .try_get_nonempty_tag_value("msg-param-sub-plan")?
                    .to_owned(),
                sub_plan_name: source
                    .try_get_nonempty_tag_value("msg-param-sub-plan-name")?
                    .to_owned(),
                num_gifted_months: source.try_get_number("msg-param-gift-months")?,
            },
            // submysterygift, anonsubmysterygift:
            // this precedes a wave of subgift/anonsubgift messages.
            // "AleMogul is gifting 100 Tier 1 Subs to NymN's community!
            // They've gifted a total of 5688 in the channel!"
            // msg-param-mass-gift-count - amount of gifts in this bulk, e.g. 100 above
            // msg-param-sender-count - total amount gifted, e.g. 5688 above
            //  - this seems to be missing if sender
            // msg-param-sub-plan (1000, 2000 or 3000 for the three sub tiers)

            // 274598607 is the user ID of "AnAnonymousGifter"
            // the dorky syntax here instead of a normal match is to accomodate the special case
            // for the submysterygift
            _ if (sender.id == "274598607" && event_id == "submysterygift")
                || event_id == "anonsubmysterygift" =>
            {
                UserNoticeEvent::AnonSubMysteryGift {
                    mass_gift_count: source.try_get_number("msg-param-mass-gift-count")?,
                    sub_plan: source
                        .try_get_nonempty_tag_value("msg-param-sub-plan")?
                        .to_owned(),
                }
            }
            // this takes over all other cases of submysterygift.
            "submysterygift" => UserNoticeEvent::SubMysteryGift {
                mass_gift_count: source.try_get_number("msg-param-mass-gift-count")?,
                sender_total_gifts: source.try_get_number("msg-param-sender-count")?,
                sub_plan: source
                    .try_get_nonempty_tag_value("msg-param-sub-plan")?
                    .to_owned(),
            },
            // giftpaidupgrade, anongiftpaidupgrade:
            // When a user commits to continue the gift sub by another user (or an anonymous gifter).
            // sender is the user continuing the gift sub.
            // note anongiftpaidupgrade actually occurs, unlike anonsubgift
            //
            // these params are present when the upgrade is part of a promotion, e.g. Subtember 2018
            // msg-param-promo-gift-total (number of gifts by the sending user in the specified promotion)
            // msg-param-promo-name (name of the promo, e.g. Subtember 2018)
            //
            // only for giftpaidupgrade:
            //   msg-param-sender-login - login name of user who gifted this user originally
            //   msg-param-sender-name - display name of user who gifted this user originally
            "giftpaidupgrade" => UserNoticeEvent::GiftPaidUpgrade {
                gifter_login: source
                    .try_get_nonempty_tag_value("msg-param-sender-login")?
                    .to_owned(),
                gifter_name: source
                    .try_get_nonempty_tag_value("msg-param-sender-name")?
                    .to_owned(),
                promotion: SubGiftPromo::parse_if_present(&source)?,
            },
            "anongiftpaidupgrade" => UserNoticeEvent::AnonGiftPaidUpgrade {
                promotion: SubGiftPromo::parse_if_present(&source)?,
            },

            // ritual
            // A user is new in a channel and uses the rituals feature to send a message letting
            // the chat know they are new.
            // "<Sender> is new to <Channel>'s chat! Say hello!"
            // msg-param-ritual-name - only valid value: "new_chatter"
            "ritual" => UserNoticeEvent::Ritual {
                ritual_name: source
                    .try_get_nonempty_tag_value("msg-param-ritual-name")?
                    .to_owned(),
            },

            // bitsbadgetier
            // When a user cheers and earns himself a new bits badge with that cheer
            // (e.g. they just cheered more than/exactly 10000 bits in total,
            // and just earned themselves the 10k bits badge)
            // msg-param-threshold - specifies the bits threshold, e.g. in the above example 10000
            "bitsbadgetier" => UserNoticeEvent::BitsBadgeTier {
                threshold: source
                    .try_get_number::<u64>("msg-param-threshold")?
                    .to_owned(),
            },

            // there are more events that are just not documented and not implemented yet. see above.
            _ => UserNoticeEvent::Unknown,
        };

        let message_text = source.params.get(1).cloned(); // can also be None
        let emotes = if let Some(message_text) = &message_text {
            source.try_get_emotes("emotes", message_text)?
        } else {
            vec![]
        };

        Ok(UserNoticeMessage {
            channel_login: source.try_get_channel_login()?.to_owned(),
            channel_id: source.try_get_nonempty_tag_value("room-id")?.to_owned(),
            sender,
            message_text,
            system_message: source.try_get_nonempty_tag_value("system-msg")?.to_owned(),
            event,
            event_id,
            badge_info: source.try_get_badges("badge-info")?,
            badges: source.try_get_badges("badges")?,
            emotes,
            name_color: source.try_get_color("color")?,
            message_id: source.try_get_nonempty_tag_value("id")?.to_owned(),
            server_timestamp: source.try_get_timestamp("tmi-sent-ts")?.to_owned(),
            source,
        })
    }
}

impl From<UserNoticeMessage> for IRCMessage {
    fn from(msg: UserNoticeMessage) -> IRCMessage {
        msg.source
    }
}

#[cfg(test)]
mod tests {
    use crate::message::twitch::{Badge, Emote, RGBColor, TwitchUserBasics};
    use crate::message::{IRCMessage, SubGiftPromo, UserNoticeEvent, UserNoticeMessage};
    use chrono::{TimeZone, Utc};
    use std::convert::TryFrom;
    use std::ops::Range;

    #[test]
    pub fn test_sub() {
        let src = "@badge-info=subscriber/0;badges=subscriber/0,premium/1;color=;display-name=fallenseraphhh;emotes=;flags=;id=2a9bea11-a80a-49a0-a498-1642d457f775;login=fallenseraphhh;mod=0;msg-id=sub;msg-param-cumulative-months=1;msg-param-months=0;msg-param-should-share-streak=0;msg-param-sub-plan-name=Channel\\sSubscription\\s(xqcow);msg-param-sub-plan=Prime;room-id=71092938;subscriber=1;system-msg=fallenseraphhh\\ssubscribed\\swith\\sTwitch\\sPrime.;tmi-sent-ts=1582685713242;user-id=224005980;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            UserNoticeMessage {
                channel_login: "xqcow".to_owned(),
                channel_id: "71092938".to_owned(),
                sender: TwitchUserBasics {
                    id: "224005980".to_owned(),
                    login: "fallenseraphhh".to_owned(),
                    name: "fallenseraphhh".to_owned(),
                },
                message_text: None,
                system_message: "fallenseraphhh subscribed with Twitch Prime.".to_owned(),
                event: UserNoticeEvent::SubOrResub {
                    is_resub: false,
                    cumulative_months: 1,
                    streak_months: None,
                    sub_plan: "Prime".to_owned(),
                    sub_plan_name: "Channel Subscription (xqcow)".to_owned(),
                },
                event_id: "sub".to_owned(),
                badge_info: vec![Badge {
                    name: "subscriber".to_owned(),
                    version: "0".to_owned(),
                }],
                badges: vec![
                    Badge {
                        name: "subscriber".to_owned(),
                        version: "0".to_owned(),
                    },
                    Badge {
                        name: "premium".to_owned(),
                        version: "1".to_owned(),
                    }
                ],
                emotes: vec![],
                name_color: None,
                message_id: "2a9bea11-a80a-49a0-a498-1642d457f775".to_owned(),
                server_timestamp: Utc.timestamp_millis(1582685713242),
                source: irc_message,
            }
        )
    }

    #[test]
    pub fn test_resub() {
        let src = "@badge-info=subscriber/2;badges=subscriber/0,battlerite_1/1;color=#0000FF;display-name=Gutrin;emotes=1035663:0-3;flags=;id=e0975c76-054c-4954-8cb0-91b8867ec1ca;login=gutrin;mod=0;msg-id=resub;msg-param-cumulative-months=2;msg-param-months=0;msg-param-should-share-streak=1;msg-param-streak-months=2;msg-param-sub-plan-name=Channel\\sSubscription\\s(xqcow);msg-param-sub-plan=1000;room-id=71092938;subscriber=1;system-msg=Gutrin\\ssubscribed\\sat\\sTier\\s1.\\sThey've\\ssubscribed\\sfor\\s2\\smonths,\\scurrently\\son\\sa\\s2\\smonth\\sstreak!;tmi-sent-ts=1581713640019;user-id=21156217;user-type= :tmi.twitch.tv USERNOTICE #xqcow :xqcL";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            UserNoticeMessage {
                channel_login: "xqcow".to_owned(),
                channel_id: "71092938".to_owned(),
                sender: TwitchUserBasics {
                    id: "21156217".to_owned(),
                    login: "gutrin".to_owned(),
                    name: "Gutrin".to_owned(),
                },
                message_text: Some("xqcL".to_owned()),
                system_message: "Gutrin subscribed at Tier 1. They've subscribed for 2 months, currently on a 2 month streak!".to_owned(),
                event: UserNoticeEvent::SubOrResub {
                    is_resub: true,
                    cumulative_months: 2,
                    streak_months: Some(2),
                    sub_plan: "1000".to_owned(),
                    sub_plan_name: "Channel Subscription (xqcow)".to_owned(),
                },
                event_id: "resub".to_owned(),
                badge_info: vec![Badge {
                    name: "subscriber".to_owned(),
                    version: "2".to_owned(),
                }],
                badges: vec![
                    Badge {
                        name: "subscriber".to_owned(),
                        version: "0".to_owned(),
                    },
                    Badge {
                        name: "battlerite_1".to_owned(),
                        version: "1".to_owned(),
                    }
                ],
                emotes: vec![
                    Emote {
                        id: "1035663".to_owned(),
                        char_range: Range { start: 0, end: 4 },
                        code: "xqcL".to_owned(),
                    }
                ],
                name_color: Some(RGBColor {
                    r: 0x00,
                    g: 0x00,
                    b: 0xFF,
                }),
                message_id: "e0975c76-054c-4954-8cb0-91b8867ec1ca".to_owned(),
                server_timestamp: Utc.timestamp_millis(1581713640019),
                source: irc_message,
            }
        )
    }

    #[test]
    pub fn test_resub_no_share_streak() {
        let src = "@badge-info=;badges=premium/1;color=#8A2BE2;display-name=rene_rs;emotes=;flags=;id=ca1f02fb-77ec-487d-a9b3-bc4bfef2fe8b;login=rene_rs;mod=0;msg-id=resub;msg-param-cumulative-months=11;msg-param-months=0;msg-param-should-share-streak=0;msg-param-sub-plan-name=Channel\\sSubscription\\s(xqcow);msg-param-sub-plan=Prime;room-id=71092938;subscriber=0;system-msg=rene_rs\\ssubscribed\\swith\\sTwitch\\sPrime.\\sThey've\\ssubscribed\\sfor\\s11\\smonths!;tmi-sent-ts=1590628650446;user-id=171356987;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message.clone()).unwrap();

        assert_eq!(
            msg,
            UserNoticeMessage {
                channel_login: "xqcow".to_owned(),
                channel_id: "71092938".to_owned(),
                sender: TwitchUserBasics {
                    id: "171356987".to_owned(),
                    login: "rene_rs".to_owned(),
                    name: "rene_rs".to_owned(),
                },
                message_text: None,
                system_message:
                    "rene_rs subscribed with Twitch Prime. They've subscribed for 11 months!"
                        .to_owned(),
                event: UserNoticeEvent::SubOrResub {
                    is_resub: true,
                    cumulative_months: 11,
                    streak_months: None,
                    sub_plan: "Prime".to_owned(),
                    sub_plan_name: "Channel Subscription (xqcow)".to_owned(),
                },
                event_id: "resub".to_owned(),
                badge_info: vec![],
                badges: vec![Badge {
                    name: "premium".to_owned(),
                    version: "1".to_owned(),
                },],
                emotes: vec![],
                name_color: Some(RGBColor {
                    r: 0x8A,
                    g: 0x2B,
                    b: 0xE2,
                }),
                message_id: "ca1f02fb-77ec-487d-a9b3-bc4bfef2fe8b".to_owned(),
                server_timestamp: Utc.timestamp_millis(1590628650446),
                source: irc_message,
            }
        )
    }

    #[test]
    pub fn test_raid() {
        let src = "@badge-info=;badges=glhf-pledge/1;color=#FF69B4;display-name=iamelisabete;emotes=;flags=;id=bb99dda7-3736-4583-9114-52aa11b23d17;login=iamelisabete;mod=0;msg-id=raid;msg-param-displayName=iamelisabete;msg-param-login=iamelisabete;msg-param-profileImageURL=https://static-cdn.jtvnw.net/jtv_user_pictures/cae3ca63-510d-4715-b4ce-059dcf938978-profile_image-70x70.png;msg-param-viewerCount=430;room-id=71092938;subscriber=0;system-msg=430\\sraiders\\sfrom\\siamelisabete\\shave\\sjoined!;tmi-sent-ts=1594517796120;user-id=155874595;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.sender,
            TwitchUserBasics {
                id: "155874595".to_owned(),
                login: "iamelisabete".to_owned(),
                name: "iamelisabete".to_owned(),
            }
        );
        assert_eq!(msg.event, UserNoticeEvent::Raid {
            viewer_count: 430,
            profile_image_url: "https://static-cdn.jtvnw.net/jtv_user_pictures/cae3ca63-510d-4715-b4ce-059dcf938978-profile_image-70x70.png".to_owned(),
        });
    }

    #[test]
    pub fn test_subgift() {
        let src = "@badge-info=;badges=sub-gifter/50;color=;display-name=AdamAtReflectStudios;emotes=;flags=;id=e21409b1-d25d-4a1a-b5cf-ef27d8b7030e;login=adamatreflectstudios;mod=0;msg-id=subgift;msg-param-gift-months=1;msg-param-months=2;msg-param-origin-id=da\\s39\\sa3\\see\\s5e\\s6b\\s4b\\s0d\\s32\\s55\\sbf\\sef\\s95\\s60\\s18\\s90\\saf\\sd8\\s07\\s09;msg-param-recipient-display-name=qatarking24xd;msg-param-recipient-id=236653628;msg-param-recipient-user-name=qatarking24xd;msg-param-sender-count=0;msg-param-sub-plan-name=Channel\\sSubscription\\s(xqcow);msg-param-sub-plan=1000;room-id=71092938;subscriber=0;system-msg=AdamAtReflectStudios\\sgifted\\sa\\sTier\\s1\\ssub\\sto\\sqatarking24xd!;tmi-sent-ts=1594583782376;user-id=211711554;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::SubGift {
                is_sender_anonymous: false,
                cumulative_months: 2,
                recipient: TwitchUserBasics {
                    id: "236653628".to_owned(),
                    login: "qatarking24xd".to_owned(),
                    name: "qatarking24xd".to_owned(),
                },
                sub_plan: "1000".to_owned(),
                sub_plan_name: "Channel Subscription (xqcow)".to_owned(),
                num_gifted_months: 1,
            }
        )
    }

    #[test]
    pub fn test_subgift_ananonymousgifter() {
        let src = "@badge-info=;badges=;color=;display-name=AnAnonymousGifter;emotes=;flags=;id=62c3fd39-84cc-452a-9096-628a5306633a;login=ananonymousgifter;mod=0;msg-id=subgift;msg-param-fun-string=FunStringThree;msg-param-gift-months=1;msg-param-months=13;msg-param-origin-id=da\\s39\\sa3\\see\\s5e\\s6b\\s4b\\s0d\\s32\\s55\\sbf\\sef\\s95\\s60\\s18\\s90\\saf\\sd8\\s07\\s09;msg-param-recipient-display-name=Dot0422;msg-param-recipient-id=151784015;msg-param-recipient-user-name=dot0422;msg-param-sub-plan-name=Channel\\sSubscription\\s(xqcow);msg-param-sub-plan=1000;room-id=71092938;subscriber=0;system-msg=An\\sanonymous\\suser\\sgifted\\sa\\sTier\\s1\\ssub\\sto\\sDot0422!\\s;tmi-sent-ts=1594495108936;user-id=274598607;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::SubGift {
                is_sender_anonymous: true,
                cumulative_months: 13,
                recipient: TwitchUserBasics {
                    id: "151784015".to_owned(),
                    login: "dot0422".to_owned(),
                    name: "Dot0422".to_owned(),
                },
                sub_plan: "1000".to_owned(),
                sub_plan_name: "Channel Subscription (xqcow)".to_owned(),
                num_gifted_months: 1,
            }
        )
    }

    #[test]
    pub fn test_anonsubgift() {
        // note there are no anonsubgift messages being sent on Twitch IRC as of the time of writing this.
        // so I created a fake one that matches what the announcement said they would be like (in theory),
        let src = "@badge-info=;badges=;color=;display-name=xQcOW;emotes=;flags=;id=e21409b1-d25d-4a1a-b5cf-ef27d8b7030e;login=xqcow;mod=0;msg-id=anonsubgift;msg-param-gift-months=1;msg-param-months=2;msg-param-origin-id=da\\s39\\sa3\\see\\s5e\\s6b\\s4b\\s0d\\s32\\s55\\sbf\\sef\\s95\\s60\\s18\\s90\\saf\\sd8\\s07\\s09;msg-param-recipient-display-name=qatarking24xd;msg-param-recipient-id=236653628;msg-param-recipient-user-name=qatarking24xd;msg-param-sender-count=0;msg-param-sub-plan-name=Channel\\sSubscription\\s(xqcow);msg-param-sub-plan=1000;room-id=71092938;subscriber=0;system-msg=An\\sanonymous\\sgifter\\sgifted\\sa\\sTier\\s1\\ssub\\sto\\sqatarking24xd!;tmi-sent-ts=1594583782376;user-id=71092938;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::SubGift {
                is_sender_anonymous: true,
                cumulative_months: 2,
                recipient: TwitchUserBasics {
                    id: "236653628".to_owned(),
                    login: "qatarking24xd".to_owned(),
                    name: "qatarking24xd".to_owned(),
                },
                sub_plan: "1000".to_owned(),
                sub_plan_name: "Channel Subscription (xqcow)".to_owned(),
                num_gifted_months: 1,
            }
        )
    }

    #[test]
    pub fn test_submysterygift() {
        let src = "@badge-info=;badges=sub-gifter/50;color=;display-name=AdamAtReflectStudios;emotes=;flags=;id=049e6371-7023-4fca-8605-7dec60e72e12;login=adamatreflectstudios;mod=0;msg-id=submysterygift;msg-param-mass-gift-count=20;msg-param-origin-id=1f\\sbe\\sbb\\s4a\\s81\\s9a\\s65\\sd1\\s4b\\s77\\sf5\\s23\\s16\\s4a\\sd3\\s13\\s09\\se7\\sbe\\s55;msg-param-sender-count=100;msg-param-sub-plan=1000;room-id=71092938;subscriber=0;system-msg=AdamAtReflectStudios\\sis\\sgifting\\s20\\sTier\\s1\\sSubs\\sto\\sxQcOW's\\scommunity!\\sThey've\\sgifted\\sa\\stotal\\sof\\s100\\sin\\sthe\\schannel!;tmi-sent-ts=1594583777669;user-id=211711554;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::SubMysteryGift {
                mass_gift_count: 20,
                sender_total_gifts: 100,
                sub_plan: "1000".to_owned(),
            }
        )
    }

    #[test]
    pub fn test_submysterygift_ananonymousgifter() {
        let src = "@badge-info=;badges=;color=;display-name=AnAnonymousGifter;emotes=;flags=;id=8db97752-3dee-460b-9001-e925d0e2ba5b;login=ananonymousgifter;mod=0;msg-id=submysterygift;msg-param-mass-gift-count=10;msg-param-origin-id=13\\s33\\sed\\sc0\\sef\\sa0\\s7b\\s9b\\s48\\s59\\scb\\scc\\se4\\s39\\s7b\\s90\\sf9\\s54\\s75\\s66;msg-param-sub-plan=1000;room-id=71092938;subscriber=0;system-msg=An\\sanonymous\\suser\\sis\\sgifting\\s10\\sTier\\s1\\sSubs\\sto\\sxQcOW's\\scommunity!;tmi-sent-ts=1585447099603;user-id=274598607;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::AnonSubMysteryGift {
                mass_gift_count: 10,
                sub_plan: "1000".to_owned(),
            }
        )
    }

    #[test]
    pub fn test_anonsubmysterygift() {
        // again, this is never emitted on IRC currently. So this test case is a made-up
        // modification of a subgift type message.
        let src = "@badge-info=;badges=;color=;display-name=xQcOW;emotes=;flags=;id=8db97752-3dee-460b-9001-e925d0e2ba5b;login=xqcow;mod=0;msg-id=anonsubmysterygift;msg-param-mass-gift-count=15;msg-param-origin-id=13\\s33\\sed\\sc0\\sef\\sa0\\s7b\\s9b\\s48\\s59\\scb\\scc\\se4\\s39\\s7b\\s90\\sf9\\s54\\s75\\s66;msg-param-sub-plan=2000;room-id=71092938;subscriber=0;system-msg=An\\sanonymous\\suser\\sis\\sgifting\\s10\\sTier\\s1\\sSubs\\sto\\sxQcOW's\\scommunity!;tmi-sent-ts=1585447099603;user-id=71092938;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::AnonSubMysteryGift {
                mass_gift_count: 15,
                sub_plan: "2000".to_owned(),
            }
        )
    }

    #[test]
    pub fn test_giftpaidupgrade_no_promo() {
        let src = "@badge-info=subscriber/2;badges=subscriber/2;color=#00FFF5;display-name=CrazyCrackAnimal;emotes=;flags=;id=7006f242-a45c-4e07-83b3-11f9c6d1ee28;login=crazycrackanimal;mod=0;msg-id=giftpaidupgrade;msg-param-sender-login=stridezgum;msg-param-sender-name=Stridezgum;room-id=71092938;subscriber=1;system-msg=CrazyCrackAnimal\\sis\\scontinuing\\sthe\\sGift\\sSub\\sthey\\sgot\\sfrom\\sStridezgum!;tmi-sent-ts=1594518849459;user-id=86082877;user-type= :tmi.twitch.tv USERNOTICE #xqcow";

        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::GiftPaidUpgrade {
                gifter_login: "stridezgum".to_owned(),
                gifter_name: "Stridezgum".to_owned(),
                promotion: None,
            }
        )
    }

    #[test]
    pub fn test_giftpaidupgrade_with_promo() {
        // I can't find any real examples for this type of message, so this is a made-up test case
        // (the same one as above, but with two tags added)
        let src = "@badge-info=subscriber/2;badges=subscriber/2;color=#00FFF5;display-name=CrazyCrackAnimal;emotes=;flags=;id=7006f242-a45c-4e07-83b3-11f9c6d1ee28;login=crazycrackanimal;mod=0;msg-id=giftpaidupgrade;msg-param-sender-login=stridezgum;msg-param-sender-name=Stridezgum;msg-param-promo-name=TestSubtember2020;msg-param-promo-gift-total=4003;room-id=71092938;subscriber=1;system-msg=CrazyCrackAnimal\\sis\\scontinuing\\sthe\\sGift\\sSub\\sthey\\sgot\\sfrom\\sStridezgum!\\sbla\\sbla\\bla\\sstuff\\sabout\\spromo\\shere;tmi-sent-ts=1594518849459;user-id=86082877;user-type= :tmi.twitch.tv USERNOTICE #xqcow";

        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::GiftPaidUpgrade {
                gifter_login: "stridezgum".to_owned(),
                gifter_name: "Stridezgum".to_owned(),
                promotion: Some(SubGiftPromo {
                    promo_name: "TestSubtember2020".to_owned(),
                    total_gifts: 4003,
                }),
            }
        )
    }

    #[test]
    pub fn test_anongiftpaidupgrade_no_promo() {
        let src = "@badge-info=subscriber/1;badges=subscriber/0,premium/1;color=#8A2BE2;display-name=samura1jack_ttv;emotes=;flags=;id=144ee636-0c1d-404e-8b29-35449a045a7e;login=samura1jack_ttv;mod=0;msg-id=anongiftpaidupgrade;room-id=71092938;subscriber=1;system-msg=samura1jack_ttv\\sis\\scontinuing\\sthe\\sGift\\sSub\\sthey\\sgot\\sfrom\\san\\sanonymous\\suser!;tmi-sent-ts=1594327421732;user-id=102707709;user-type= :tmi.twitch.tv USERNOTICE #xqcow";

        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::AnonGiftPaidUpgrade { promotion: None }
        )
    }

    #[test]
    pub fn test_anongiftpaidupgrade_with_promo() {
        // I can't find any real examples for this type of message, so this is a made-up test case
        // (the same one as above, but with two tags added)
        let src = "@badge-info=subscriber/1;badges=subscriber/0,premium/1;color=#8A2BE2;display-name=samura1jack_ttv;emotes=;flags=;id=144ee636-0c1d-404e-8b29-35449a045a7e;msg-param-promo-name=TestSubtember2020;msg-param-promo-gift-total=4003;login=samura1jack_ttv;mod=0;msg-id=anongiftpaidupgrade;room-id=71092938;subscriber=1;system-msg=samura1jack_ttv\\sis\\scontinuing\\sthe\\sGift\\sSub\\sthey\\sgot\\sfrom\\san\\sanonymous\\suser!\\sbla\\sbla\\bla\\sstuff\\sabout\\spromo\\shere;tmi-sent-ts=1594327421732;user-id=102707709;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::AnonGiftPaidUpgrade {
                promotion: Some(SubGiftPromo {
                    promo_name: "TestSubtember2020".to_owned(),
                    total_gifts: 4003,
                })
            }
        )
    }

    #[test]
    pub fn test_ritual() {
        let src = "@badge-info=;badges=;color=;display-name=SevenTest1;emotes=30259:0-6;id=37feed0f-b9c7-4c3a-b475-21c6c6d21c3d;login=seventest1;mod=0;msg-id=ritual;msg-param-ritual-name=new_chatter;room-id=6316121;subscriber=0;system-msg=Seventoes\\sis\\snew\\shere!;tmi-sent-ts=1508363903826;turbo=0;user-id=131260580;user-type= :tmi.twitch.tv USERNOTICE #seventoes :HeyGuys";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::Ritual {
                ritual_name: "new_chatter".to_owned()
            }
        )
    }

    #[test]
    pub fn test_bitsbadgetier() {
        let src = "@badge-info=subscriber/2;badges=subscriber/2,bits/1000;color=#FF4500;display-name=whoopiix;emotes=;flags=;id=d2b32a02-3071-4c52-b2ce-bc3716acdc44;login=whoopiix;mod=0;msg-id=bitsbadgetier;msg-param-threshold=1000;room-id=71092938;subscriber=1;system-msg=bits\\sbadge\\stier\\snotification;tmi-sent-ts=1594520403813;user-id=104252055;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.event,
            UserNoticeEvent::BitsBadgeTier { threshold: 1000 }
        )
    }

    #[test]
    pub fn test_unknown() {
        // just an example of an undocumented type of message that we don't parse currently.
        let src = "@badge-info=;badges=sub-gifter/50;color=;display-name=AdamAtReflectStudios;emotes=;flags=;id=7f1336e4-f84a-4510-809d-e57bf50af0cc;login=adamatreflectstudios;mod=0;msg-id=rewardgift;msg-param-domain=pride_megacommerce_2020;msg-param-selected-count=100;msg-param-total-reward-count=100;msg-param-trigger-amount=20;msg-param-trigger-type=SUBGIFT;room-id=71092938;subscriber=0;system-msg=AdamAtReflectStudios's\\sGift\\sshared\\srewards\\sto\\s100\\sothers\\sin\\sChat!;tmi-sent-ts=1594583778756;user-id=211711554;user-type= :tmi.twitch.tv USERNOTICE #xqcow";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(msg.event, UserNoticeEvent::Unknown)
    }

    #[test]
    pub fn test_sneaky_action_invalid_emote_tag() {
        // See https://github.com/twitchdev/issues/issues/175
        let src = r"@badge-info=subscriber/23;badges=moderator/1,subscriber/12;color=#19E6E6;display-name=randers;emotes=25:7-11,23-27/499:29-30;flags=;id=8c2918c2-adf4-4208-a554-8a72d016de70;login=randers;mod=1;msg-id=resub;msg-param-cumulative-months=23;msg-param-months=0;msg-param-should-share-streak=1;msg-param-streak-months=23;msg-param-sub-plan-name=look\sat\sthose\sshitty\semotes,\srip\s$5\sLUL;msg-param-sub-plan=1000;room-id=11148817;subscriber=1;system-msg=randers\ssubscribed\sat\sTier\s1.\sThey've\ssubscribed\sfor\s23\smonths,\scurrently\son\sa\s23\smonth\sstreak!;tmi-sent-ts=1595497450553;user-id=40286300;user-type=mod :tmi.twitch.tv USERNOTICE #pajlada :ACTION Kappa TEST TEST Kappa :)";
        let irc_message = IRCMessage::parse(src).unwrap();
        let msg = UserNoticeMessage::try_from(irc_message).unwrap();

        assert_eq!(
            msg.message_text,
            Some("ACTION Kappa TEST TEST Kappa :)".to_owned())
        );
        assert_eq!(
            msg.emotes,
            vec![
                Emote {
                    id: "25".to_owned(),
                    char_range: Range { start: 7, end: 12 },
                    code: " Kapp".to_owned(),
                },
                Emote {
                    id: "25".to_owned(),
                    char_range: Range { start: 23, end: 28 },
                    code: " Kapp".to_owned(),
                },
                Emote {
                    id: "499".to_owned(),
                    char_range: Range { start: 29, end: 31 },
                    code: " :".to_owned(),
                },
            ]
        )
    }
}
