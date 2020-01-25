use crate::message::IRCParseError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TransportError {
    #[error("{0}")]
    IOError(#[from] std::io::Error),
    #[error("{0}")]
    IRCParseError(#[from] IRCParseError),
}
