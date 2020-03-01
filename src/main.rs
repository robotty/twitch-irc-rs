#[macro_use]
extern crate rental;

use crate::client::config::{ClientConfig, StaticLoginCredentials};
use crate::client::transport::TCPTransport;
use crate::client::TwitchIRCClient;
use futures::prelude::*;
use std::env;

pub mod client;
pub mod message;
pub mod util;

#[tokio::main]
pub async fn main() {
    run().await.unwrap();
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let config = ClientConfig {
        login_credentials: StaticLoginCredentials {
            login: "randers01".to_owned(),
            token: Some(
                env::var("IRC_RS_TOKEN")
                    .expect("IRC_RS_TOKEN environment variable missing")
                    .to_owned(),
            ),
        },
        ..Default::default()
    };

    let mut client: TwitchIRCClient<TCPTransport, StaticLoginCredentials> =
        TwitchIRCClient::new(config);

    let mut incoming_messages = client.incoming_messages.take().unwrap();

    let join_handle = tokio::spawn(async move {
        while let Some(message) = incoming_messages.next().await {
            log::info!("Received: {:?}", message);
        }
    });

    client.join("pajlada".to_owned()).await;

    let (res,) = futures::join!(join_handle);

    res.unwrap();

    Ok(())
}
