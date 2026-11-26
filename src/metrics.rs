use crate::MetricsConfig;
use prometheus::{
    Counter, CounterVec, IntGauge, Opts, register_counter_vec_with_registry,
    register_counter_with_registry, register_int_gauge_vec_with_registry,
};

#[derive(Clone)]
pub struct MetricsBundle {
    pub messages_received: CounterVec,
    pub messages_sent: CounterVec,
    /// Pre-resolved handle for `twitchirc_channels{type="wanted"}`.
    pub channels_wanted: IntGauge,
    /// Pre-resolved handle for `twitchirc_channels{type="server"}`.
    pub channels_server: IntGauge,
    /// Pre-resolved handle for `twitchirc_connections{state="initializing"}`.
    pub connections_initializing: IntGauge,
    /// Pre-resolved handle for `twitchirc_connections{state="open"}`.
    pub connections_open: IntGauge,
    pub connections_failed: CounterVec,
    pub connections_created: Counter,
}

impl MetricsBundle {
    pub fn new(config: &MetricsConfig) -> Option<MetricsBundle> {
        let (const_labels, metrics_registry) = match config {
            MetricsConfig::Disabled => {
                return None;
            }
            MetricsConfig::Enabled {
                constant_labels,
                metrics_registry,
            } => (
                constant_labels,
                match metrics_registry {
                    Some(metrics_registry) => metrics_registry,
                    None => prometheus::default_registry(),
                },
            ),
        };

        let messages_received = register_counter_vec_with_registry!(
            Opts::new(
                "twitchirc_messages_received",
                "Number of raw IRC messages received by the Twitch IRC server since start of the client, across all connections."
            ).const_labels(const_labels.clone()),
            &["command"],
            metrics_registry
        ).unwrap();

        let messages_sent = register_counter_vec_with_registry!(
            Opts::new(
                "twitchirc_messages_sent",
                "Number of raw IRC messages sent to the Twitch IRC server since start of the client, across all connections."
            ).const_labels(const_labels.clone()),
            &["command"],
            metrics_registry
        ).unwrap();

        let channels = register_int_gauge_vec_with_registry!(
            Opts::new(
                "twitchirc_channels",
                "Number of channels the client is currently joined to"
            )
            .const_labels(const_labels.clone()),
            &["type"],
            metrics_registry
        )
        .unwrap();

        let connections = register_int_gauge_vec_with_registry!(
            Opts::new(
                "twitchirc_connections",
                "Number of connections currently active on this client"
            )
            .const_labels(const_labels.clone()),
            &["state"],
            metrics_registry
        )
        .unwrap();

        let connections_failed = register_counter_vec_with_registry!(
            Opts::new(
                "twitchirc_connections_failed",
                "Number of times a connection has failed since the start of this client"
            )
            .const_labels(const_labels.clone()),
            &["cause"],
            metrics_registry
        )
        .unwrap();

        let connections_created = register_counter_with_registry!(
            Opts::new(
                "twitchirc_connections_created",
                "Number of times a new connection was made to add it to the connection pool (since the start of this client)"
            )
            .const_labels(const_labels.clone()),
            metrics_registry
        )
        .unwrap();

        // Pre-resolve fixed-label gauge handles once so `update_metrics()` writes directly to the
        // underlying atomics instead of locking and hashing label slices on every update
        // (the parent `IntGaugeVec`s remain registered with `metrics_registry`).
        let channels_wanted = channels.with_label_values(&["wanted"]);
        let channels_server = channels.with_label_values(&["server"]);
        let connections_initializing = connections.with_label_values(&["initializing"]);
        let connections_open = connections.with_label_values(&["open"]);

        Some(MetricsBundle {
            messages_received,
            messages_sent,
            channels_wanted,
            channels_server,
            connections_initializing,
            connections_open,
            connections_failed,
            connections_created,
        })
    }
}
