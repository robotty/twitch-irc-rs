use crate::client::TwitchIRCClient;
use crate::config::{ClientConfig, StaticLoginCredentials};
use crate::transport::tcp::TCPTransport;
use futures::prelude::*;
use std::env;

pub mod client;
pub mod config;
pub mod connection;
pub mod message;
pub mod transport;

#[tokio::main]
pub async fn main() {
    run().await.unwrap();
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let token = env::var("IRC_RS_TOKEN")
        .expect("IRC_RS_TOKEN environment variable missing")
        .to_owned();
    let config = ClientConfig {
        login_credentials: StaticLoginCredentials::new("randers01".to_owned(), Some(token)),
        ..Default::default()
    };

    let mut client =
        TwitchIRCClient::<TCPTransport<StaticLoginCredentials>, StaticLoginCredentials>::new(
            config,
        );

    let mut incoming_messages = client.incoming_messages.take().unwrap();

    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.next().await {
            log::info!("Received: {:?}", message);
        }
    });

    log::info!("joining randers...");
    let res = client.join("randers".to_owned()).await;
    log::info!("joined? {:?}", res);

    let (res,) = futures::join!(join_handle);

    res?;

    Ok(())
}
