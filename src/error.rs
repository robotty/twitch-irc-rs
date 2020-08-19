use crate::login::LoginCredentials;
use crate::message::IRCParseError;
use crate::transport::Transport;
use thiserror::Error;

/// Errors that can occur while trying to execute some action on a `TwitchIRCClient`.
#[derive(Error, Debug)]
pub enum Error<T: Transport, L: LoginCredentials> {
    /// Underlying transport failed to connect
    #[error("Underlying transport failed to connect: {0}")]
    ConnectError(T::ConnectError),
    /// Error received from incoming stream of messages
    #[error("Error received from incoming stream of messages: {0}")]
    IncomingError(T::IncomingError),
    /// Error received while trying to send message(s) out
    #[error("Error received while trying to send message(s) out: {0}")]
    OutgoingError(T::OutgoingError),
    /// Incoming message was not valid IRC
    #[error("Incoming message was not valid IRC: {0}")]
    IRCParseError(IRCParseError),
    /// Failed to get login credentials to log in with
    #[error("Failed to get login credentials to log in with: {0}")]
    LoginError(L::Error),
    /// Received RECONNECT command by IRC server
    #[error("Received RECONNECT command by IRC server")]
    ReconnectCmd,
    /// Did not receive a PONG back after sending PING
    #[error("Did not receive a PONG back after sending PING")]
    PingTimeout,
    /// Remote server unexpectedly closed connection
    #[error("Remote server unexpectedly closed connection")]
    ConnectionClosed,
}
