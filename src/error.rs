use crate::login::LoginCredentials;
use crate::message::IRCParseError;
use crate::transport::Transport;
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur while trying to execute some action on a `TwitchIRCClient`.
#[derive(Error, Debug)]
pub enum Error<T: Transport, L: LoginCredentials> {
    /// Underlying transport failed to connect
    #[error("Underlying transport failed to connect: {0}")]
    ConnectError(Arc<T::ConnectError>),
    /// Underlying transport failed to connect in time
    #[error("Underlying transport failed to connect: Connect timed out")]
    ConnectTimeout,
    /// Error received from incoming stream of messages
    #[error("Error received from incoming stream of messages: {0}")]
    IncomingError(Arc<T::IncomingError>),
    /// Error received while trying to send message(s) out
    #[error("Error received while trying to send message(s) out: {0}")]
    OutgoingError(Arc<T::OutgoingError>),
    /// Incoming message was not valid IRC
    #[error("Incoming message was not valid IRC: {0}")]
    IRCParseError(IRCParseError),
    /// Failed to get login credentials to log in with
    #[error("Failed to get login credentials to log in with: {0}")]
    LoginError(Arc<L::Error>),
    /// Received RECONNECT command by IRC server
    #[error("Received RECONNECT command by IRC server")]
    ReconnectCmd,
    /// Did not receive a PONG back after sending PING
    #[error("Did not receive a PONG back after sending PING")]
    PingTimeout,
    /// Remote server unexpectedly closed connection
    #[error("Remote server unexpectedly closed connection")]
    RemoteUnexpectedlyClosedConnection,
}

impl<T: Transport, L: LoginCredentials> Clone for Error<T, L> {
    fn clone(&self) -> Self {
        match self {
            Error::ConnectError(e) => Error::ConnectError(Arc::clone(e)),
            Error::ConnectTimeout => Error::ConnectTimeout,
            Error::IncomingError(e) => Error::IncomingError(Arc::clone(e)),
            Error::OutgoingError(e) => Error::OutgoingError(Arc::clone(e)),
            Error::IRCParseError(e) => Error::IRCParseError(*e),
            Error::LoginError(e) => Error::LoginError(Arc::clone(e)),
            Error::ReconnectCmd => Error::ReconnectCmd,
            Error::PingTimeout => Error::PingTimeout,
            Error::RemoteUnexpectedlyClosedConnection => Error::RemoteUnexpectedlyClosedConnection,
        }
    }
}

impl<T: Transport, L: LoginCredentials> Error<T, L> {
    /// Low-cardinality `snake_case` identifier for this error variant, used for metrics and
    /// structured logging.
    pub(crate) fn cause_label(&self) -> &'static str {
        match self {
            Error::ConnectError(_) => "connect_error",
            Error::ConnectTimeout => "connect_timeout",
            Error::IncomingError(_) => "incoming_error",
            Error::OutgoingError(_) => "outgoing_error",
            Error::IRCParseError(_) => "irc_parse_error",
            Error::LoginError(_) => "login_error",
            Error::ReconnectCmd => "reconnect_cmd",
            Error::PingTimeout => "ping_timeout",
            Error::RemoteUnexpectedlyClosedConnection => "remote_closed",
        }
    }
}
