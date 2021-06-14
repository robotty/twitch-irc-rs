# twitch-irc-rs

[![Rust CI status](https://github.com/robotty/twitch-irc-rs/workflows/Rust/badge.svg)](https://github.com/robotty/twitch-irc-rs/actions)
[![Crates.io](https://img.shields.io/crates/v/twitch-irc)](https://crates.io/crates/twitch-irc)
[![Docs.rs](https://docs.rs/twitch-irc/badge.svg)](https://docs.rs/twitch-irc)

My attempt at a Twitch IRC library for the Rust programming language, using the recently stabilized async rust traits/language features.

Example usage (This is the `simple_listener` example, see `examples/simple_listener.rs` and run it with `cargo run --example simple_listener`):

```rust
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::TwitchIRCClient;
use twitch_irc::{ClientConfig, SecureTCPTransport};

#[tokio::main]
pub async fn main() {
    // default configuration is to join chat as anonymous.
    let config = ClientConfig::default();
    let (mut incoming_messages, client) =
        TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config);

    // first thing you should do: start consuming incoming messages,
    // otherwise they will back up.
    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.recv().await {
            println!("Received message: {:?}", message);
        }
    });

    // join a channel
    client.join("sodapoppin".to_owned());

    // keep the tokio executor alive.
    // If you return instead of waiting the background task will exit.
    join_handle.await.unwrap();
}
```

Check out the [documentation on docs.rs](https://docs.rs/twitch-irc) for more details.
