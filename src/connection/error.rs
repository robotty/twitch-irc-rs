use crate::config::LoginCredentials;
use crate::message::commands::ServerMessageParseError;
use crate::message::IRCParseError;
use crate::transport::Transport;
use derivative::Derivative;
use thiserror::Error;

// note: if you #[derive(Error, std::fmt::Debug)] directly
// it will complain that T and L don't implement std::fmt::Debug.
// using derivative is a cheap fix to avoid having work around this via
// other bulkier ways
#[derive(Error, Derivative)]
#[derivative(Debug)]
pub enum ConnectionError<T: Transport<L>, L: LoginCredentials> {
    #[error("{0:?}")]
    ConnectError(T::ConnectError),
    #[error("{0:?}")]
    IncomingError(T::IncomingError),
    #[error("{0:?}")]
    OutgoingError(T::OutgoingError),
    #[error("{0:?}")]
    IRCParseError(IRCParseError),
    #[error("{0:?}")]
    ServerMessageParseError(ServerMessageParseError),
    #[error("{0:?}")]
    LoginError(L::Error),
    #[error("Received RECONNECT command by IRC server")]
    ReconnectCmd,
    #[error("Did not receive a PONG back after sending PING")]
    PingTimeout,
    #[error("Outgoing messages stream closed")]
    ConnectionClosed,
}
