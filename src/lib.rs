//! Connect to Twitch chat from a Rust application.

mod client;
mod config;
mod connection;
pub mod login;
pub mod message;
mod transport;

pub use client::TwitchIRCClient;
pub use config::ClientConfig;
pub use connection::error::ConnectionError;
pub use connection::Connection;

#[cfg(feature = "transport-tcp")]
pub use transport::tcp::TCPTransport;
#[cfg(feature = "transport-wss")]
pub use transport::websocket::WSSTransport;
pub use transport::Transport;
