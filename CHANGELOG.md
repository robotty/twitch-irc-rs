# Changelog
Version numbers follow [Semantic Versioning](https://semver.org/).

## Unreleased

- Minor: Updated to tokio v0.3. (#48)
- Minor: Added a new config option to specify connect timeout. (#48)
- Bugfix: Fixed client sporadically locking up as a result of the TLS connection setup not having a timeout. (#48)

## v0.2.1

- Bugfix: Fixed compile error when default features were disabled. (#21)

## v0.2.0

- Breaking: Renamed `Error::ClientClosed` to `Error::RemoteUnexpectedlyClosedConnection` to clarify intent
- Minor: Added `event_id` to `UserNoticeMessage` to allow parsing undocumented types of events.
- Minor: Client previously returned `Error::ClientClosed` in cases where that error wasnt appropriate (when the client was closed due to some other error that had occurred previously) - the error that caused a connection to close down is now kept as long as needed and every time the connection is used after this error, a clone of the original cause is returned.
- Bugfix: Fixed `UserStateMessage` not being exported (pub).

## v0.1.2

- Bugfix: Include all features in the documentation on docs.rs.

## v0.1.1

- Minor: Gracefully work around a [Twitch bug](https://github.com/twitchdev/issues/issues/104) regarding emote indices.
- Bugfix: Fixed possible issue with messages getting sent out in the wrong order after connection setup.

## v0.1.0

This was the initial release.
