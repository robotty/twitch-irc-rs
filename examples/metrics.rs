use std::collections::HashMap;

use axum::routing::get;
use axum::Router;
use prometheus::TextEncoder;
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::TwitchIRCClient;
use twitch_irc::{ClientConfig, MetricsConfig, SecureTCPTransport};

const WEBSERVER_LISTEN_ADDR: &str = "127.0.0.1:3000";

// This example demonstrates usage of the `metrics-collection` feature flag.
// `metrics-collection` enables a set of metrics to be exported from the client.
// See the documentation on `ClientConfig` and `MetricsConfig` for details.
//
// Creates a web server at 127.0.0.1:3000. GET http://127.0.0.1:3000/metrics to see the current set of metrics
// exported by the client.
#[tokio::main]
pub async fn main() {
    tracing_subscriber::fmt::init();

    let config = ClientConfig {
        // Enable metrics collection.
        metrics_config: MetricsConfig::Enabled {
            // These labels are added to all metrics exported by the client.
            // If your app has multiple twitch-irc clients, you can differentiate
            // them this way, e.g. client=listener, client=second-thing, etc.
            // Here we just use some exemplary extra data. You can add anything you want.
            constant_labels: {
                let mut labels = HashMap::new();
                labels.insert("app".to_owned(), "metrics-example".to_owned());
                labels.insert("version".to_owned(), env!("CARGO_PKG_VERSION").to_owned());
                labels
            },
            // `None` specifies that metrics are to be registered with the global registry from the prometheus crate
            metrics_registry: None,
        },
        // rest of the config is default
        ..ClientConfig::default()
    };
    let (mut incoming_messages, client) =
        TwitchIRCClient::<SecureTCPTransport, StaticLoginCredentials>::new(config);

    let message_handler = tokio::spawn(async move {
        while let Some(message) = incoming_messages.recv().await {
            tracing::info!("Received message: {:?}", message);
        }
    });
    client.join("sodapoppin".to_owned()).unwrap();

    let web_app = Router::new().route("/metrics", get(get_metrics));
    let web_server = tokio::spawn(
        axum::Server::bind(&WEBSERVER_LISTEN_ADDR.parse().unwrap())
            .serve(web_app.into_make_service()),
    );
    tracing::info!("Listening for requests at {WEBSERVER_LISTEN_ADDR}");

    web_server.await.unwrap().unwrap();
    message_handler.await.unwrap();
}

// Web request handler for GET /metrics
pub async fn get_metrics() -> String {
    // Export all metrics from the global registry from the prometheus crate
    TextEncoder.encode_to_string(&prometheus::gather()).unwrap()
}
