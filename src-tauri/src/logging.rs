//! Logging. Lifted from Modz (NoWayJA/Modz, MIT) logging.rs: a daily-rolling file, stdout,
//! and an in-memory tail the UI can read. Timestamps are real (chrono) rather than epoch
//! seconds. Never log setting values or secrets; log keys and names.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::LazyLock;

use parking_lot::RwLock;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, fmt};

const MAX_LINES: usize = 500;
const FILE_NAME: &str = "terrace.log";

static BUFFER: LazyLock<RwLock<VecDeque<String>>> =
    LazyLock::new(|| RwLock::new(VecDeque::with_capacity(MAX_LINES)));

/// Keeps the non-blocking writer alive for the life of the app. Managed by Tauri.
pub struct LogGuard(pub WorkerGuard);

struct MemoryLayer;

impl<S> Layer<S> for MemoryLayer
where
    S: tracing::Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);
        let line = format!(
            "{} {:<5} [{}] {}",
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            event.metadata().level(),
            event.metadata().target(),
            visitor.message
        );
        let mut buffer = BUFFER.write();
        if buffer.len() >= MAX_LINES {
            buffer.pop_front();
        }
        buffer.push_back(line);
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl MessageVisitor {
    fn push(&mut self, field: &tracing::field::Field, value: String) {
        if field.name() == "message" {
            if self.message.is_empty() {
                self.message = value;
            } else {
                self.message = format!("{value} {}", self.message);
            }
        } else if self.message.is_empty() {
            self.message = format!("{}={value}", field.name());
        } else {
            self.message.push_str(&format!(" {}={value}", field.name()));
        }
    }
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        self.push(field, format!("{value:?}"));
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        self.push(field, value.to_string());
    }
}

/// Initialise logging into `log_dir`. Returns `None` (and logs nothing to file) when the
/// directory cannot be created; the app still runs.
pub fn init(log_dir: &Path) -> Option<WorkerGuard> {
    if let Err(error) = std::fs::create_dir_all(log_dir) {
        eprintln!(
            "terrace: cannot create log directory {}: {error}",
            log_dir.display()
        );
        return None;
    }

    let appender = tracing_appender::rolling::daily(log_dir, FILE_NAME);
    let (non_blocking, guard) = tracing_appender::non_blocking(appender);

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("terrace=debug,terrace_lib=debug,tauri=info,kube=info"));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true);
    let console_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_ansi(true)
        .with_target(true);

    if tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(console_layer)
        .with(MemoryLayer)
        .try_init()
        .is_err()
    {
        eprintln!("terrace: a global tracing subscriber was already set");
    }

    Some(guard)
}

/// The most recent `count` lines, newest first.
pub fn recent(count: usize) -> Vec<String> {
    BUFFER.read().iter().rev().take(count).cloned().collect()
}
