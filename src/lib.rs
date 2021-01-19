#![warn(missing_docs)]
//! Connect to Twitch chat from a Rust application.
//!
//! This library supports the modern stdlib futures and runs using the `tokio` runtime.
//!
//! # Getting started
//!
//! The central feature of this library is the `TwitchIRCClient` which connects to Twitch IRC
//! for you using a pool of connections and handles all the important bits. Here is a minimal
//! example to get you started:
//!
//! ```no_run
//! use twitch_irc::login::StaticLoginCredentials;
//! use twitch_irc::ClientConfig;
//! use twitch_irc::TCPTransport;
//! use twitch_irc::TwitchIRCClient;
//!
//! #[tokio::main]
//! pub async fn main() {
//!     // default configuration is to join chat as anonymous.
//!     let config = ClientConfig::default();
//!     let (mut incoming_messages, client) =
//!         TwitchIRCClient::<TCPTransport, StaticLoginCredentials>::new(config);
//!
//!     // first thing you should do: start consuming incoming messages,
//!     // otherwise they will back up.
//!     let join_handle = tokio::spawn(async move {
//!         while let Some(message) = incoming_messages.recv().await {
//!             println!("Received message: {:?}", message);
//!         }
//!     });
//!
//!     // join a channel
//!     client.join("sodapoppin".to_owned());
//!
//!     // keep the tokio executor alive.
//!     // If you return instead of waiting the background task will exit.
//!     join_handle.await.unwrap();
//! }
//! ```
//!
//! The above example connects to chat anonymously and listens to messages coming to the channel `sodapoppin`.
//!
//! # Features
//!
//! * Simple API
//! * Integrated connection pool, new connections will be made based on your application's demand
//!   (based on amount of channels joined as well as number of outgoing messages)
//! * Automatic reconnect of failed connections, automatically re-joins channels
//! * Rate limiting of new connections
//! * Support for refreshing login tokens, see below
//! * Fully parses all message types (see [`ServerMessage`](message/enum.ServerMessage.html)
//!   for all supported types)
//! * Can connect using both plain TLS-secured socket as well as secure websocket
//! * No unsafe code
//! * Feature flags to reduce compile time and binary size
//!
//! # Send messages
//!
//! To send messages, use the `TwitchIRCClient` handle you get from `TwitchIRCClient::new`.
//!
//! ```no_run
//! # use twitch_irc::login::StaticLoginCredentials;
//! # use twitch_irc::ClientConfig;
//! # use twitch_irc::TCPTransport;
//! # use twitch_irc::TwitchIRCClient;
//! #
//! # #[tokio::main]
//! # async fn main() {
//! # let config = ClientConfig::default();
//! # let (mut incoming_messages, client) = TwitchIRCClient::<TCPTransport, StaticLoginCredentials>::new(config);
//! client.say("a_channel".to_owned(), "Hello world!".to_owned()).await.unwrap();
//! # }
//! ```
//!
//! The `TwitchIRCClient` handle can also be cloned and then used from multiple threads.
//!
//! See the documentation on [`TwitchIRCClient`](struct.TwitchIRCClient.html)
//! for the possible methods.
//!
//! # Receive and handle messages
//!
//! Incoming messages are [`ServerMessage`](message/enum.ServerMessage.html)s. You can use a match
//! block to differentiate between the possible server messages:
//!
//! ```no_run
//! # use twitch_irc::message::ServerMessage;
//! # use tokio::sync::mpsc;
//! #
//! # #[tokio::main]
//! # async fn main() {
//! # let mut incoming_messages: mpsc::UnboundedReceiver<ServerMessage> = unimplemented!();
//! while let Some(message) = incoming_messages.recv().await {
//!      match message {
//!          ServerMessage::Privmsg(msg) => {
//!              println!("(#{}) {}: {}", msg.channel_login, msg.sender.name, msg.message_text);
//!          },
//!          ServerMessage::Whisper(msg) => {
//!              println!("(w) {}: {}", msg.sender.name, msg.message_text);
//!          },
//!          _ => {}
//!      }
//! }
//! # }
//! ```
//!
//! # Logging in
//!
//! `twitch_irc` ships with [`StaticLoginCredentials`](login/struct.StaticLoginCredentials.html)
//! and [`RefreshingLoginCredentials`](login/struct.RefreshingLoginCredentials.html).
//!
//! For simple cases, `StaticLoginCredentials` fulfills all needs:
//!
//! ```
//! use twitch_irc::login::StaticLoginCredentials;
//! use twitch_irc::ClientConfig;
//!
//! let login_name = "your_bot_name".to_owned();
//! let oauth_token = "u0i05p6kbswa1w72wu1h1skio3o20t".to_owned();
//!
//! let config = ClientConfig::new_simple(
//!     StaticLoginCredentials::new(login_name, Some(oauth_token))
//! );
//! ```
//!
//! However for most applications it is strongly recommended to have your login token automatically
//! refreshed when it expires. For this, enable the `refreshing-token` feature flag, and use
//! [`RefreshingLoginCredentials`](login/struct.RefreshingLoginCredentials.html), for example
//! like this:
//!
//! ```no_run
//! use async_trait::async_trait;
//! use twitch_irc::login::{RefreshingLoginCredentials, TokenStorage, UserAccessToken};
//! use twitch_irc::ClientConfig;
//! use std::path::Path;
//!
//! #[derive(Debug)]
//! struct CustomTokenStorage {
//!     // fields...
//! }
//!
//! #[async_trait]
//! impl TokenStorage for CustomTokenStorage {
//!     type LoadError = std::io::Error; // or some other error
//!     type UpdateError = std::io::Error;
//!
//!     async fn load_token(&mut self) -> Result<UserAccessToken, Self::LoadError> {
//!         // Load the currently stored token from the storage.
//!         todo!()
//!     }
//!
//!     async fn update_token(&mut self, token: &UserAccessToken) -> Result<(), Self::UpdateError> {
//!         // Called after the token was updated successfully, to save the new token.
//!         // After `update_token()` completes, the `load_token()` method should then return
//!         // that token for future invocations
//!         todo!()
//!     }
//! }
//!
//! let login_name = "your_bot_name".to_owned();
//! // these credentials can be generated for your app at https://dev.twitch.tv/console/apps
//! let client_id = "rrbau1x7hl2ssz78nd2l32ns9jrx2w".to_owned();
//! let client_secret = "m6nuam2b2zgn2fw8actt8hwdummz1g".to_owned();
//! let storage = CustomTokenStorage { /* ... */ };
//!
//! let config = ClientConfig::new_simple(
//!     RefreshingLoginCredentials::new(login_name, client_id, client_secret, storage)
//! );
//! // then create your client and use it
//! ```
//!
//! `RefreshingLoginCredentials` just needs an implementation of `TokenStorage` that depends
//! on your application, to retrieve the token or update it. For example, you might put the token
//! in a config file you overwrite, some extra file for secrets, or a database.
//!
//! # Close the client
//!
//! To close the client, drop all clones of the `TwitchIRCClient` handle. The client will shut down
//! and end the stream of incoming messages once all processing is done.
//!
//! # Feature flags
//!
//! This library has these optional feature toggles:
//! * **`transport-tcp`** enables `TCPTransport`, to connect using a plain TLS socket using the
//!   normal IRC protocol.
//! * **`transport-wss`** enables `WSSTransport` to connect using the Twitch-specific websocket
//!   method.
//! * **`refreshing-token`** enables
//!   [`RefreshingLoginCredentials`](login/struct.RefreshingLoginCredentials.html) (see above).
//! * **`metrics-collection`** enables a set of metrics to be exported from the client. See the
//!   documentation on `ClientConfig` for details.
//!
//! By default, only `transport-tcp` is enabled.

mod client;
mod config;
mod connection;
mod error;
pub mod login;
pub mod message;
mod transport;

pub use client::TwitchIRCClient;
pub use config::ClientConfig;
pub use error::Error;

#[cfg(feature = "transport-tcp")]
pub use transport::tcp::TCPTransport;
#[cfg(feature = "transport-wss")]
pub use transport::websocket::WSSTransport;
pub use transport::Transport;
