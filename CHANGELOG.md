# Changelog
Version numbers follow [Semantic Versioning](https://semver.org/).

## v4.0.0

- Breaking: Updated `metrics` to version 0.17.

## v3.0.1

- Bugfix: Fixed `FollowersOnlyMode` enum not being exported from the crate. (#135)

## v3.0.0

- Breaking: Transports were refactored slightly:  
  Renamed `twitch_irc::TCPTransport` to `SecureTCPTransport`, added `PlainTCPTransport` for plain-text IRC connections.  
  Renamed `twitch_irc::WSSTransport` to `SecureWSTransport`, added `PlainWSTransport` for plain-text IRC-over-WebSocket-connections.  
  Refactored feature flags: This crate used to only have the `transport-tcp` and `transport-wss` feature flags. The following is the new list of feature flags relevant to transports:
    - `transport-tcp` enables `PlainTCPTransport`
    - `transport-tcp-native-tls` enables `SecureTCPTransport` using OS-native TLS functionality (and using the root certificates configured in your operating system).
    - `transport-tcp-rustls-native-roots` enables `SecureTCPTransport` using rustls, but still using the root certificates configured in your operating system.
    - `transport-tcp-rustls-webpki-roots` enables `SecureTCPTransport` using rustls with root certificates provided by [`webpki-roots`](https://github.com/ctz/webpki-roots) (Mozilla's root certificates). This is the most portable since it does not rely on OS-specific functionality.
    - `transport-ws` (notice this is now `ws` instead of `wss`) - Enables `PlainWSTransport`
    - `transport-ws-native-tls` - Enables `SecureWSTransport` using native TLS (same as above)
    - `transport-ws-rustls-webpki-roots` - Enables `SecureWSTransport` using rustls with Mozilla's root certificates (same as above)
  
  Some accompanying items have also been made `pub` in the crate.
- Breaking: Updated `metrics` to version 0.16.
- Minor: Added `timeout`, `untimeout`, `ban` and `unban` methods to `TwitchIRCClient` (#110)
- Minor: Added `serde` feature, adding the ability to serialize or deserialize the command structs using serde. (#120)
- Minor: Metrics are no longer initialized, undoing the change introduced with v2.2.0 (#129)

## v2.2.0

- Bugfix: Fixed fields on `UserAccessToken` being all private, preventing library users from constructing the type (as part of the `RefreshingLoginCredentials` system). (#101, #103)
- Reduce the amount of dependencies used. (#96)
- Update `metrics` dependency to v0.14. Metrics are now registered with a description when the
  client is created. (#97)
- Chore: Fix all the clippy warnings in the project.

## v2.1.0

- Minor: Added `say_in_response` and `reply_to_privmsg` methods to `TwitchIRCClient` (#84)

## v2.0.0

- Updates to tokio v1.0 (#75)

## v1.0.0

- Reverts to tokio v0.2 (#56)

## v0.3.0
This release was later yanked because the tokio 0.3 upgrade was incomplete. Multiple versions of tokio
were specified in the dependencies.

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
