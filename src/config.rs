use crate::login::{LoginCredentials, StaticLoginCredentials};
#[cfg(feature = "metrics-collection")]
use std::borrow::Cow;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Configures settings for a `TwitchIRCClient`.
#[derive(Debug)]
pub struct ClientConfig<L: LoginCredentials> {
    /// Gets a set of credentials every time the client needs to log in on a new connection.
    /// See [`LoginCredentials`] for details.
    pub login_credentials: L,

    /// A new connection will automatically be created if a channel is joined and all
    /// currently established connections have joined at least this many channels.
    pub max_channels_per_connection: usize,

    /// A new connection will automatically be created if any message is to be sent
    /// and all currently established connections have recently sent more than this many
    /// messages (time interval is defined by `max_waiting_messages_duration_window`)
    pub max_waiting_messages_per_connection: usize,

    /// We assume messages to be "waiting" for this amount of time after sending them out, e.g.
    /// typically 100 or 150 milliseconds (purely a value that has been measured/observed,
    /// not documented or fixed in any way)
    pub time_per_message: Duration,

    /// rate-limits the opening of new connections. By default this is constructed with 1 permit
    /// only, which means connections cannot be opened in parallel. If this is set to more than 1
    /// permit, then that many connections can be opened in parallel.
    ///
    /// This is designed to be wrapped in an Arc to allow it to be shared between multiple
    /// TwitchIRCClient instances.
    pub connection_rate_limiter: Arc<Semaphore>,

    /// Allow a new connection to be made after this period has elapsed. By default this is set
    /// to 2 seconds, and combined with the permits=1 of the semaphore, allows one connection
    /// to be made every 2 seconds.
    ///
    /// More specifically, after taking the permit from the semaphore, the permit will be put
    /// back after this period has elapsed.
    pub new_connection_every: Duration,

    /// Imposes a general timeout for new connections. This is in place in addition to possible
    /// operating system timeouts (E.g. for new TCP connections), since additional "connect" work
    /// takes place after the TCP connection is opened, e.g. to set up TLS or perform a WebSocket
    /// handshake. Default value: 20 seconds.
    pub connect_timeout: Duration,

    /// Set this to `None` to disable metrics collection for this client.
    ///
    /// If this is set to `Some(value)`, then metrics are collected from this client using
    /// the `metrics` crate under the `twitch_irc_` prefix. Because multiple clients
    /// may coexist at the same time, this string should be picked to be unique in your application.
    /// The client will label all metrics it publishes using this identifier string.
    /// The specific client is then identified using the `client` label on all metrics below.
    ///
    /// Currently exported metrics:
    /// * `twitch_irc_messages_received` with label `command` counts all incoming messages. (Counter)
    ///
    /// * `twitch_irc_messages_sent` counts messages sent out, with a `command` label. (Counter)
    ///
    /// * `twitch_irc_channels` with `type=allocated/confirmed` counts how many channels
    ///   you are joined to (Gauge). Allocated channels are joins that passed through the `TwitchIRCClient`
    ///   but may be waiting e.g. for the connection to finish connecting. Once a
    ///   confirmation response is received by Twitch that the channel was joined successfully,
    ///   that channel is additionally `confirmed`.
    ///
    /// * `twitch_irc_connections` counts how many connections this client has in use (Gauge).
    ///    The label `state=initializing/open` identifies how many connections are
    ///    in the process of connecting (`initializing`) vs how many connections are already established (`open`).
    ///
    /// * `twitch_irc_reconnects` counts every time a connection fails (Counter). Note however, depending
    ///   on conditions e.g. how many channels were joined on that channel, the connection may not
    ///   actually have been reconnected (despite the name `twitch_irc_reconnects`).
    ///   If other connections have enough capacity left to join the channels from the failed
    ///   connection, then no new connection will be made.
    #[cfg(feature = "metrics-collection")]
    pub metrics_identifier: Option<Cow<'static, str>>,
}

impl<L: LoginCredentials> ClientConfig<L> {
    /// Create a new configuration from the given login credentials, with all other configuration
    /// options being default.
    pub fn new_simple(login_credentials: L) -> ClientConfig<L> {
        ClientConfig {
            login_credentials,
            max_channels_per_connection: 90,

            max_waiting_messages_per_connection: 5,
            time_per_message: Duration::from_millis(150),

            // 1 connection every 2 seconds seems to work well
            connection_rate_limiter: Arc::new(Semaphore::new(1)),
            new_connection_every: Duration::from_secs(2),
            connect_timeout: Duration::from_secs(20),

            #[cfg(feature = "metrics-collection")]
            metrics_identifier: None,
        }
    }
}

impl Default for ClientConfig<StaticLoginCredentials> {
    fn default() -> ClientConfig<StaticLoginCredentials> {
        ClientConfig::new_simple(StaticLoginCredentials::anonymous())
    }
}
