# twitch-irc-rs

My attempt at a Twitch IRC library for the Rust programming language, using the recently stabilized async rust traits/language features.

Example usage:

```rust
env_logger::init();

let config = ClientConfig {
    login_credentials: StaticLoginCredentials::new("randers01".to_owned(), Some("abcdef123456".to_owned())),
    ..Default::default()
};

let mut client =
    TwitchIRCClient::<TCPTransport<StaticLoginCredentials>, StaticLoginCredentials>::new(
        config,
    );

let mut incoming_messages = client.incoming_messages.take().unwrap();

let join_handle = tokio::spawn(async move {
    while let Some(message) = incoming_messages.next().await {
        log::info!("Received message: {:?}", message);
    }
});

log::info!("joining a channel...");
let res = client.join("forsen".to_owned()).await;
log::info!("Channel join result: {:?}", res);

let (res,) = futures::join!(join_handle);
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

TODO things that will be finished soon-ish:
- Implementation of twitch-imposed rate limits (PRIVMSG, Whisper)
- More specific ServerMessage types (e.g. twitch-specific types like Privmsg, Whisper, Clearchat, Clearmsg, etc.). Currently only the bare-bones set of types are implemented (the ones that are needed for the operation of the library)
