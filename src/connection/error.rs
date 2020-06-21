use crate::config::LoginCredentials;
use crate::message::commands::ServerMessageParseError;
use crate::message::IRCParseError;
use crate::transport::Transport;
use std::fmt::{Debug, Display};
use thiserror::Error;

// TC is short for Transport::ConnectionError,
// TI for Transport::IncomingError,
// TO for Transport::OutgoingError,
// L for LoginCredentials::Error
#[derive(Error, Debug)]
pub enum ConnectionError<TC, TI, TO, L>
where
    TC: Send + Sync + Display + Debug,
    TI: Send + Sync + Display + Debug,
    TO: Send + Sync + Display + Debug,
    L: Send + Sync + Display + Debug,
{
    #[error("{0}")]
    ConnectError(TC),
    #[error("{0}")]
    IncomingError(TI),
    #[error("{0}")]
    OutgoingError(TO),
    #[error("{0}")]
    IRCParseError(IRCParseError),
    #[error("{0}")]
    ServerMessageParseError(ServerMessageParseError),
    #[error("{0}")]
    LoginError(L),
    #[error("Received RECONNECT command by IRC server")]
    ReconnectCmd(),
    #[error("Did not receive a PONG back after sending PING")]
    PingTimeout(),
    #[error("Outgoing messages stream closed")]
    ConnectionClosed(),
}

pub type ConnErr<T, L> = ConnectionError<
    <T as Transport<L>>::ConnectError,
    <T as Transport<L>>::IncomingError,
    <T as Transport<L>>::OutgoingError,
    <L as LoginCredentials>::Error,
>;
