use crate::login::LoginCredentials;
use crate::message::IRCParseError;
use crate::transport::Transport;
use thiserror::Error;

/// Errors that can occur inside a `TwitchIRCClient` and `Connection`.
#[derive(Error, Debug)]
pub enum Error<T: Transport, L: LoginCredentials> {
    #[error("{0}")]
    ConnectError(T::ConnectError),
    #[error("{0}")]
    IncomingError(T::IncomingError),
    #[error("{0}")]
    OutgoingError(T::OutgoingError),
    #[error("{0}")]
    IRCParseError(IRCParseError),
    #[error("{0}")]
    LoginError(L::Error),
    #[error("Received RECONNECT command by IRC server")]
    ReconnectCmd,
    #[error("Did not receive a PONG back after sending PING")]
    PingTimeout,
    #[error("Remote server unexpectedly closed connection")]
    ConnectionClosed,
}
