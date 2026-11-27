use async_trait::async_trait;
use either::Either;
use futures_util::sink::Sink;
use futures_util::stream::{FusedStream, Stream};
use std::collections::HashSet;
use std::convert::Infallible;
use std::hint::black_box;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, mpsc};
use twitch_irc::login::StaticLoginCredentials;
use twitch_irc::message::{IRCMessage, IRCParseError, IRCPrefix, IRCTags, ServerMessage};
use twitch_irc::transport::Transport;
use twitch_irc::{ClientConfig, RateLimiter, TwitchIRCClient};

#[cfg(feature = "metrics-collection")]
use std::collections::HashMap;
#[cfg(feature = "metrics-collection")]
use twitch_irc::MetricsConfig;

type IncomingItem = Result<IRCMessage, Either<Infallible, IRCParseError>>;

#[derive(Debug)]
struct SyncIncoming {
    rx: Mutex<mpsc::UnboundedReceiver<IncomingItem>>,
    terminated: bool,
}

impl Stream for SyncIncoming {
    type Item = IncomingItem;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if this.terminated {
            return Poll::Ready(None);
        }
        match this.rx.get_mut().unwrap().poll_recv(cx) {
            Poll::Ready(Some(item)) => Poll::Ready(Some(item)),
            Poll::Ready(None) => {
                this.terminated = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl FusedStream for SyncIncoming {
    fn is_terminated(&self) -> bool {
        self.terminated
    }
}

#[derive(Debug)]
struct EchoOutgoing {
    incoming_tx: mpsc::UnboundedSender<IncomingItem>,
}

impl EchoOutgoing {
    fn dummy_user_prefix() -> IRCPrefix {
        IRCPrefix::Full {
            nick: "justinfan12345".to_owned(),
            user: Some("justinfan12345".to_owned()),
            host: Some("justinfan12345.tmi.twitch.tv".to_owned()),
        }
    }
}

impl Sink<IRCMessage> for EchoOutgoing {
    type Error = Infallible;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: IRCMessage) -> Result<(), Self::Error> {
        match item.command.as_str() {
            "JOIN" => {
                if let Some(channel) = item.params.first()
                    && !channel.starts_with("#unconfirmed_")
                {
                    let msg = IRCMessage::new(
                        IRCTags::new(),
                        Some(Self::dummy_user_prefix()),
                        "JOIN".to_owned(),
                        vec![channel.clone()],
                    );
                    let _ = self.incoming_tx.send(Ok(msg));
                }
            }
            "PART" => {
                if let Some(channel) = item.params.first() {
                    let msg = IRCMessage::new(
                        IRCTags::new(),
                        Some(Self::dummy_user_prefix()),
                        "PART".to_owned(),
                        vec![channel.clone()],
                    );
                    let _ = self.incoming_tx.send(Ok(msg));
                }
            }
            "PING" => {
                let msg =
                    IRCMessage::new_simple("PONG".to_owned(), vec!["tmi.twitch.tv".to_owned()]);
                let _ = self.incoming_tx.send(Ok(msg));
            }
            _ => {}
        }
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

#[derive(Debug)]
struct EchoJoinTransport {
    incoming: SyncIncoming,
    outgoing: EchoOutgoing,
}

#[async_trait]
impl Transport for EchoJoinTransport {
    type ConnectError = Infallible;
    type IncomingError = Infallible;
    type OutgoingError = Infallible;
    type Incoming = SyncIncoming;
    type Outgoing = EchoOutgoing;

    async fn new() -> Result<Self, Self::ConnectError> {
        let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
        Ok(EchoJoinTransport {
            incoming: SyncIncoming {
                rx: Mutex::new(incoming_rx),
                terminated: false,
            },
            outgoing: EchoOutgoing { incoming_tx },
        })
    }

    fn split(self) -> (Self::Incoming, Self::Outgoing) {
        (self.incoming, self.outgoing)
    }
}

fn make_config() -> ClientConfig<StaticLoginCredentials> {
    ClientConfig {
        connection_rate_limiter: Arc::new(Semaphore::new(10_000)),
        new_connection_every: Duration::ZERO,
        join_rate_limiter: Arc::new(RateLimiter::unlimited()),
        part_rate_limiter: Arc::new(RateLimiter::unlimited()),
        #[cfg(feature = "metrics-collection")]
        metrics_config: MetricsConfig::Enabled {
            constant_labels: HashMap::new(),
            metrics_registry: Some(prometheus::Registry::new()),
        },
        ..ClientConfig::default()
    }
}

async fn drain_confirmations(
    incoming_messages: &mut mpsc::UnboundedReceiver<ServerMessage>,
    expected_joins: usize,
    expected_parts: usize,
) {
    let mut joins_seen = 0usize;
    let mut parts_seen = 0usize;
    while joins_seen < expected_joins || parts_seen < expected_parts {
        let msg = incoming_messages
            .recv()
            .await
            .expect("incoming message stream ended before confirmations arrived");
        match msg {
            ServerMessage::Join(_) => joins_seen += 1,
            ServerMessage::Part(_) => parts_seen += 1,
            _ => {}
        }
    }
}

async fn shutdown_client(
    client: TwitchIRCClient<EchoJoinTransport, StaticLoginCredentials>,
    mut incoming_messages: mpsc::UnboundedReceiver<ServerMessage>,
) {
    drop(client);
    while incoming_messages.recv().await.is_some() {}
}

struct ProbeStats {
    count: usize,
    p99: Duration,
    max: Duration,
}

async fn run_with_status_probe(
    client: &TwitchIRCClient<EchoJoinTransport, StaticLoginCredentials>,
    channels: HashSet<String>,
    probe_channel: String,
) -> (Duration, Duration, ProbeStats) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_probe = Arc::clone(&stop);
    let probe_client = client.clone();

    let probe_handle = tokio::spawn(async move {
        let mut samples = Vec::new();
        while !stop_probe.load(Ordering::Relaxed) {
            let t0 = Instant::now();
            let status = probe_client.get_channel_status(probe_channel.clone()).await;
            black_box(status);
            samples.push(t0.elapsed());
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        samples
    });

    // Yield once so the probe task starts immediately.
    tokio::task::yield_now().await;

    let start = Instant::now();
    client.set_wanted_channels(channels).unwrap();
    let enqueue_elapsed = start.elapsed();
    // `connect()` acts as an O(1) FIFO barrier on `ClientLoopWorker` so we measure full
    // server-loop processing time even if `get_channel_status` is moved off-loop later.
    client.connect().await;
    let total_elapsed = start.elapsed();

    stop.store(true, Ordering::Relaxed);
    let mut samples = probe_handle.await.unwrap();

    let stats = if samples.is_empty() {
        ProbeStats {
            count: 0,
            p99: Duration::ZERO,
            max: Duration::ZERO,
        }
    } else {
        samples.sort_unstable();
        let p99_idx = ((samples.len() - 1) * 99) / 100;
        ProbeStats {
            count: samples.len(),
            p99: samples[p99_idx],
            max: *samples.last().unwrap(),
        }
    };

    (enqueue_elapsed, total_elapsed, stats)
}

fn summarize_durations(mut durations: Vec<Duration>) -> (Duration, Duration, Duration) {
    durations.sort_unstable();
    let min = durations[0];
    let median = durations[durations.len() / 2];
    let max = *durations.last().unwrap();
    (min, median, max)
}

fn fmt_ms(d: Duration) -> String {
    format!("{:.2} ms", d.as_secs_f64() * 1000.0)
}

fn fmt_us(d: Duration) -> String {
    format!("{:.1} us", d.as_secs_f64() * 1_000_000.0)
}

struct BenchArgs {
    channels: Vec<usize>,
    iters: usize,
}

fn parse_args() -> BenchArgs {
    let mut channels = vec![10_000, 50_000, 100_000, 300_000];
    let mut iters = 3usize;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(val) = arg.strip_prefix("--channels=") {
            channels = val
                .split(',')
                .filter(|s| !s.is_empty())
                .map(|s| s.parse::<usize>().expect("invalid integer in --channels"))
                .collect();
        } else if arg == "--channels" {
            if let Some(val) = args.next() {
                channels = val
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.parse::<usize>().expect("invalid integer in --channels"))
                    .collect();
            }
        } else if let Some(val) = arg.strip_prefix("--iters=") {
            iters = val
                .parse::<usize>()
                .expect("invalid integer in --iters")
                .max(1);
        } else if arg == "--iters"
            && let Some(val) = args.next()
        {
            iters = val
                .parse::<usize>()
                .expect("invalid integer in --iters")
                .max(1);
        }
    }

    BenchArgs { channels, iters }
}

async fn bench_scale(n: usize, iters: usize) {
    println!(
        "\n=== n={} channels, iters={}, metrics_collection={} ===",
        n,
        iters,
        cfg!(feature = "metrics-collection")
    );

    // Build base channel set outside timed regions.
    let base_channels: HashSet<String> = (0..n).map(|i| format!("channel_{i}")).collect();
    let probe_channel = "channel_0".to_owned();

    // --- Client A: all channels confirmed ---
    let (mut incoming_a, client_a) =
        TwitchIRCClient::<EchoJoinTransport, StaticLoginCredentials>::new(make_config());

    // 1. Cold start initial join
    let t0 = Instant::now();
    client_a.set_wanted_channels(base_channels.clone()).unwrap();
    let cold_enqueue = t0.elapsed();
    client_a.connect().await;
    let cold_total = t0.elapsed();
    println!(
        "  cold_start_initial_join    : total={} (enqueue+validate={})",
        fmt_ms(cold_total),
        fmt_ms(cold_enqueue)
    );

    // 2. Drain initial join confirmations
    let drain_start = Instant::now();
    drain_confirmations(&mut incoming_a, n, 0).await;
    println!(
        "  confirm_initial_joins      : {} ({} JOINs drained)",
        fmt_ms(drain_start.elapsed()),
        n
    );

    // 3. Idle get_channel_status latency (500 hits + 500 misses)
    let idle_samples_count = 1000usize;
    let mut idle_samples = Vec::with_capacity(idle_samples_count);
    for i in 0..idle_samples_count {
        let name = if i % 2 == 0 {
            format!("channel_{}", (i / 2) % n.max(1))
        } else {
            format!("missing_{i}")
        };
        let s0 = Instant::now();
        let status = client_a.get_channel_status(name).await;
        black_box(status);
        idle_samples.push(s0.elapsed());
    }
    idle_samples.sort_unstable();
    let idle_mean = idle_samples.iter().sum::<Duration>() / (idle_samples.len() as u32);
    let idle_p99 = idle_samples[((idle_samples.len() - 1) * 99) / 100];
    let idle_max = *idle_samples.last().unwrap();
    println!(
        "  get_channel_status_idle    : mean={}, p99={}, max={} (1000 calls)",
        fmt_us(idle_mean),
        fmt_us(idle_p99),
        fmt_us(idle_max)
    );

    // 4. Steady state (no change)
    let mut no_change_totals = Vec::with_capacity(iters);
    let mut no_change_probe_p99s = Vec::with_capacity(iters);
    let mut no_change_probe_max = Duration::ZERO;
    let mut no_change_probe_counts = 0usize;
    for _ in 0..iters {
        let channels = base_channels.clone();
        let (_enq, total, probe) =
            run_with_status_probe(&client_a, channels, probe_channel.clone()).await;
        no_change_totals.push(total);
        no_change_probe_p99s.push(probe.p99);
        no_change_probe_max = no_change_probe_max.max(probe.max);
        no_change_probe_counts += probe.count;
    }
    let (min_t, med_t, max_t) = summarize_durations(no_change_totals);
    let (_, med_p99, _) = summarize_durations(no_change_probe_p99s);
    println!(
        "  steady_state_no_change     : median={}, min={}, max={} | concurrent get_channel_status p99={}, max={} ({} probes)",
        fmt_ms(med_t),
        fmt_ms(min_t),
        fmt_ms(max_t),
        fmt_ms(med_p99),
        fmt_ms(no_change_probe_max),
        no_change_probe_counts
    );

    // 5. Steady state (1% churn: 0.5% parted + 0.5% joined per iteration)
    let churn_count = (n / 200).max(1);
    let mut current_channels = base_channels;
    let mut churn_totals = Vec::with_capacity(iters);
    let mut churn_probe_p99s = Vec::with_capacity(iters);
    let mut churn_probe_max = Duration::ZERO;
    let mut churn_probe_counts = 0usize;

    for iter in 0..iters {
        let to_remove: Vec<String> = current_channels.iter().take(churn_count).cloned().collect();
        for ch in &to_remove {
            current_channels.remove(ch);
        }
        for j in 0..churn_count {
            current_channels.insert(format!("churn_{iter}_{j}"));
        }

        let channels = current_channels.clone();
        let (_enq, total, probe) =
            run_with_status_probe(&client_a, channels, probe_channel.clone()).await;
        drain_confirmations(&mut incoming_a, churn_count, churn_count).await;

        churn_totals.push(total);
        churn_probe_p99s.push(probe.p99);
        churn_probe_max = churn_probe_max.max(probe.max);
        churn_probe_counts += probe.count;
    }
    let (min_t, med_t, max_t) = summarize_durations(churn_totals);
    let (_, med_p99, _) = summarize_durations(churn_probe_p99s);
    println!(
        "  steady_state_1pct_churn    : median={}, min={}, max={} | concurrent get_channel_status p99={}, max={} ({} probes, {} churned/iter)",
        fmt_ms(med_t),
        fmt_ms(min_t),
        fmt_ms(max_t),
        fmt_ms(med_p99),
        fmt_ms(churn_probe_max),
        churn_probe_counts,
        churn_count * 2
    );

    shutdown_client(client_a, incoming_a).await;

    // --- Client B: 99% confirmed + 1% unconfirmed ---
    let unconfirmed_count = (n / 100).max(1);
    let confirmed_count = n - unconfirmed_count;
    let mut unconfirmed_set: HashSet<String> = (0..confirmed_count)
        .map(|i| format!("channel_{i}"))
        .collect();
    for i in 0..unconfirmed_count {
        unconfirmed_set.insert(format!("unconfirmed_{i}"));
    }

    let (mut incoming_b, client_b) =
        TwitchIRCClient::<EchoJoinTransport, StaticLoginCredentials>::new(make_config());
    client_b
        .set_wanted_channels(unconfirmed_set.clone())
        .unwrap();
    client_b.connect().await;
    drain_confirmations(&mut incoming_b, confirmed_count, 0).await;

    let mut unconf_totals = Vec::with_capacity(iters);
    let mut unconf_probe_p99s = Vec::with_capacity(iters);
    let mut unconf_probe_max = Duration::ZERO;
    let mut unconf_probe_counts = 0usize;

    for _ in 0..iters {
        let channels = unconfirmed_set.clone();
        let (_enq, total, probe) =
            run_with_status_probe(&client_b, channels, probe_channel.clone()).await;
        unconf_totals.push(total);
        unconf_probe_p99s.push(probe.p99);
        unconf_probe_max = unconf_probe_max.max(probe.max);
        unconf_probe_counts += probe.count;
    }
    let (min_t, med_t, max_t) = summarize_durations(unconf_totals);
    let (_, med_p99, _) = summarize_durations(unconf_probe_p99s);
    println!(
        "  steady_state_1pct_unconf   : median={}, min={}, max={} | concurrent get_channel_status p99={}, max={} ({} probes, {} unconfirmed)",
        fmt_ms(med_t),
        fmt_ms(min_t),
        fmt_ms(max_t),
        fmt_ms(med_p99),
        fmt_ms(unconf_probe_max),
        unconf_probe_counts,
        unconfirmed_count
    );

    shutdown_client(client_b, incoming_b).await;
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    println!(
        "Running set_wanted_channels baseline benchmark (channels={:?}, iters={})",
        args.channels, args.iters
    );
    for &n in &args.channels {
        bench_scale(n, args.iters).await;
    }
}
