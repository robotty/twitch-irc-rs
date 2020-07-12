use env_logger::Env;
use futures::prelude::*;
use twitch_irc::client::TwitchIRCClient;
use twitch_irc::config::ClientConfig;
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::transport::tcp::TCPTransport;

#[tokio::main]
pub async fn main() {
    env_logger::from_env(Env::default().default_filter_or("simple_listener=trace,twitch_irc=info"))
        .init();

    // default configuration is to join chat as anonymous.
    let config = ClientConfig::default();
    let mut client = TwitchIRCClient::<TCPTransport, StaticLoginCredentials>::new(config);

    // first thing you should do: start consuming incoming messages, otherwise they will
    // back up.
    let mut incoming_messages = client.incoming_messages.take().unwrap();
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
