# twitch-irc-rs

My attempt at a Twitch IRC library for the Rust programming language, using the recently stabilized async rust traits/language features.

Example usage (This is the `simple_listener` example, see `examples/simple_listener.rs` and run it with `cargo run --example simple_listener`):

```rust
use env_logger::Env;
use futures::prelude::*;
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::ClientConfig;
use twitch_irc::TCPTransport;
use twitch_irc::TwitchIRCClient;

#[tokio::main]
pub async fn main() {
    env_logger::from_env(Env::default().default_filter_or("simple_listener=trace,twitch_irc=info"))
        .init();

    // default configuration is to join chat as anonymous.
    let config = ClientConfig::default();
    let (mut incoming_messages, client) =
        TwitchIRCClient::<TCPTransport, StaticLoginCredentials>::new(config);

    // first thing you should do: start consuming incoming messages, otherwise they will
    // back up.
    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.next().await {
            log::info!("Received message: {:?}", message);
        }
    });

    // join the channel
    log::info!("Joining the channel...");
    client.join("sodapoppin".to_owned()).await.unwrap();
    log::info!("Successfully joined.");

    // keep the tokio executor alive. If you return instead of waiting the background task will exit.
    join_handle.await.unwrap();
}
```

Current features:
- Connection pool, new connections will be made based upon load
  - Will create a new connection if all existing connections have already joined 90 channels (number is configurable)
  - Will create a new connection if all connections are currently busy (if it has recently sent a lot of messages and you risk a long delay from your messages being queued up server-side)
- Automatic reconnect of failed connections
- Automatically rejoins channels if connections fail
- Modern async interface
- Automatic rate limiting of new connections
- Supports automatic token refresh for tokens that are not infinitely lived (also supports infinitely lived tokens separately)

TODO things:
- Implementation of twitch-imposed rate limits (PRIVMSG, Whisper)
- More specific ServerMessage types (e.g. twitch-specific types like Privmsg, Whisper, Clearchat, Clearmsg, etc.). Currently only the bare-bones set of types are implemented (the ones that are needed for the operation of the library)
