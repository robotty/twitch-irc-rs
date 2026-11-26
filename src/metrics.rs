use crate::MetricsConfig;
use prometheus::{
    Counter, CounterVec, Histogram, HistogramOpts, IntGauge, Opts, exponential_buckets,
    linear_buckets, register_counter_vec_with_registry, register_counter_with_registry,
    register_histogram_vec_with_registry, register_histogram_with_registry,
    register_int_gauge_vec_with_registry,
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
    pub connection_init_queued_joins: Histogram,
    /// Pre-resolved handle for `twitchirc_connection_init_joins_coalesced{reason="duplicate"}`.
    pub connection_init_duplicate_joins_dropped: Histogram,
    /// Pre-resolved handle for `twitchirc_connection_init_joins_coalesced{reason="parted"}`.
    pub connection_init_joins_cancelled_by_part: Histogram,
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

        // 0 (send/whisper-only), 1 (single-channel), 10..=100 in linear steps of 10 (covering
        // default max_channels_per_connection=90), plus exponential headroom up to 800.
        let mut init_queued_joins_buckets = vec![0.0, 1.0];
        init_queued_joins_buckets.extend(linear_buckets(10.0, 10.0, 10).unwrap());
        init_queued_joins_buckets.extend(exponential_buckets(200.0, 2.0, 3).unwrap());

        let connection_init_queued_joins = register_histogram_with_registry!(
            HistogramOpts::new(
                "twitchirc_connection_init_queued_joins",
                "Number of unique JOIN commands flushed when a connection finishes initializing"
            )
            .const_labels(const_labels.clone())
            .buckets(init_queued_joins_buckets),
            metrics_registry
        )
        .unwrap();

        // 0 (no coalescing) plus powers of two from 1 to 8192 for outlier/hot-channel bursts.
        let mut init_joins_coalesced_buckets = vec![0.0];
        init_joins_coalesced_buckets.extend(exponential_buckets(1.0, 2.0, 14).unwrap());

        let connection_init_joins_coalesced = register_histogram_vec_with_registry!(
            HistogramOpts::new(
                "twitchirc_connection_init_joins_coalesced",
                "Number of JOIN commands dropped or cancelled while a connection was initializing"
            )
            .const_labels(const_labels.clone())
            .buckets(init_joins_coalesced_buckets),
            &["reason"],
            metrics_registry
        )
        .unwrap();

        // Pre-resolve fixed-label handles once so metric updates write directly to the
        // underlying atomics instead of locking and hashing label slices on every update
        // (the parent Vecs remain registered with `metrics_registry`).
        let channels_wanted = channels.with_label_values(&["wanted"]);
        let channels_server = channels.with_label_values(&["server"]);
        let connections_initializing = connections.with_label_values(&["initializing"]);
        let connections_open = connections.with_label_values(&["open"]);
        let connection_init_duplicate_joins_dropped =
            connection_init_joins_coalesced.with_label_values(&["duplicate"]);
        let connection_init_joins_cancelled_by_part =
            connection_init_joins_coalesced.with_label_values(&["parted"]);

        Some(MetricsBundle {
            messages_received,
            messages_sent,
            channels_wanted,
            channels_server,
            connections_initializing,
            connections_open,
            connections_failed,
            connections_created,
            connection_init_queued_joins,
            connection_init_duplicate_joins_dropped,
            connection_init_joins_cancelled_by_part,
        })
    }
}
