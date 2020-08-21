# Changelog
## Unreleased

- Minor: Added `event_id` to `UserNoticeMessage` to allow parsing undocumented types of events.
- Bugfix: Fixed `UserStateMessage` not being exported (pub).

## v0.1.2

- Bugfix: Include all features in the documentation on docs.rs.

## v0.1.1

- Minor: Gracefully work around a [Twitch bug](https://github.com/twitchdev/issues/issues/104) regarding emote indices.
- Bugfix: Fixed possible issue with messages getting sent out in the wrong order after connection setup.

## v0.1.0

This was the initial release.
