# Changelog

Version numbers follow [Semantic Versioning](https://semver.org/).

## Unreleased

- Breaking: Fixed a erroneous implementation of the IRCv3 tags: This crate now no longer differentiates
  between empty and missing IRCv3 tag values (e.g. `@key` is equivalent to `@key=`). The type of the
  `IRCTags` struct has changed to hold a `HashMap<String, String>` instead of a `HashMap<String, Option<String>>`.

  Where as re-stringifying messages with the above distinction was flawless before, this information
  is now intentionally discarded during parsing. This means `@key=` becomes `@key` if the message is parsed
  and re-stringified (This is the recommended and confirming way, according to the standard).

  See also: #186 and #196
- Breaking: Removed `ban()`, `unban()`, `timeout()` and `untimeout()` since they are no longer supported by Twitch.
  They were previously deprecated in v4.1.0 (#197)
- Breaking: Fixed typo in RoomStateMessage's follower mode (was `follwers_only`, is now `followers_only`. (#200)
- Minor: Added support for reply-parent tags (#189)
- Minor: Tokens in `CredentialsPair` and `UserAccessToken` are now redacted in their `Debug` output. Same
  applies to the `client_secret` in `RefreshingLoginCredentials`. (#199)
- Minor: Added example demonstrating usage of `metrics-collection` feature. (#203)

## v5.0.1

- Minor: Removed unused features from the `chrono` dependency (#185)
- Bugfix: Upgraded dependencies to eliminate vulnerability in the `webpki` crate. (#193)

## v5.0.0

- Breaking: A lot of details regarding the metrics collection system have been reworked. (#160)
  - Switched from using the `metrics` crate to using the `promtheus` crate.
  - Usage of the new library and new config types now allows you to specify a `Registry`
    (from the `prometheus` crate) to register the metrics on, instead of being forced
    to use one global registry like with the `metrics` crate.
  - The config option `metrics_identifier` in the config has been replaced by a
    `MetricsConfig` enum, which now allows complete flexibility regarding what labels
    are added to the metrics. (Previously, only `client => a_value_chosen_by_you` could be
    added, and adding it was mandatory due to the API design I had chosen previously.)
  - This also means you can now specify any amount of additional `key => value` label pairs
    to be placed on the exported metrics. I imagine this to be useful to export any kind of
    metadata about your application/use-case.
  - The metrics have been renamed from `twitch_irc_*` to `twitchirc_*` [to align with
    the prometheus naming conventions](https://prometheus.io/docs/practices/naming/).
  - `twitch_irc_reconnects` has been renamed to `twitchirc_connections_failed` to better
    reflect what it actually counts.
  - Added `twitchirc_connections_created` as the obvious counterpart to aforementioned metric.
- Breaking: Handle `emote_sets` as `String`s since not all of them are in fact `u64`s (#162)
- Breaking: Removed `ChatClient::say_in_response` and `ChatClient::reply_to_privmsg` in favour
  of a new API design (`Chatclient::say_in_reply_to`) which does the same thing as the previous
  functions, just with a sleeker API design. (#166)
- Breaking: Removed support for parsing `HOSTTARGET` messages (Twitch has removed Host mode since 2022-10-03) (#183)
- Minor: Added `me` and `me_in_reply_to` methods to send `/me` messages. (#170)
- Minor: Implement `Clone` for `RefreshingLoginCredentials`. (#176)
- Minor: Implement `Eq` for many structs. (#177)

## v4.1.0

- Minor: Mark `ban`, `unban`, `timeout` and `untimeout` methods as deprecated (Due to Twitch removing support for these commands on 2023-02-18: https://discuss.dev.twitch.tv/t/deprecation-of-chat-commands-through-irc/40486) (#181)

## v4.0.0

- Breaking: Updated `metrics` to version 0.18. (#146)
- Breaking: Implement user login fetching via the API when using `RefreshingLoginCredentials`. (#144)
- Breaking: It was possible to trip up the channel joinery logic of the client pretty good by joining channels with a comma in them. Added a basic validation step to `TwitchIRCClient.login` and `TwitchIRCClient.set_wanted_channels` to ensure channel names are in a sane format - these two functions now return a `Result` that you will have to handle in your code. The underlying used validation function is exported for use as `twitch_irc::validate::validate_login`. (#149, #154, #156)
- Breaking: This library now no longer uses the `log` crate for logging. All logging is now done via [`tracing`](https://docs.rs/tracing). This allows you to now differentiate log messages by async task, by connection, and if configured, even by client if your application is running multiple clients. A new configuration option has been introduced for this: `config.tracing_identifier`. (#151)
- Breaking: Renamed the `refreshing-token` feature to `refreshing-token-native-tls` to reflect the fact that it pulls in the OS's native TLS library (e.g. OpenSSL/Schannel). Added the `refreshing-token-rustls-native-roots` and `refreshing-token-rustls-webpki-roots` feature flags to complement the other parts of the library where you can choose between the three options. (#153)
- Minor: Implement `Clone` for `RefreshingLoginCredentials` (#143)
- Minor: Added feature flag `transport-ws-rustls-native-roots` to allow websocket connections powered by rustls using the OS-native root certificates. (#146)
- Minor: Updated some further dependencies:
  - `async-tungstenite` `0.13` -> `0.17`
  - `rustls-native-certs` `0.5` -> `0.6`
  - `tokio-rustls` `0.22` -> `0.23`
  - `tokio-util` `0.6` -> `0.7`
  - `webpki-roots` `0.21` -> `0.22`

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
