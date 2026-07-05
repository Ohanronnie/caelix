use std::{fmt, process, sync::Once, time::Duration};

use chrono::Local;
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    registry::LookupSpan,
};

use crate::{Controller, Module, RouteDef};

const RESET: &str = "\x1b[0m";
const BRAND: &str = "\x1b[38;5;45m";
const PID: &str = "\x1b[38;5;141m";
const TIME: &str = "\x1b[38;5;245m";
const CONTEXT: &str = "\x1b[38;5;111m";
const INFO: &str = "\x1b[38;5;39m";
const WARN: &str = "\x1b[38;5;214m";
const ERROR: &str = "\x1b[38;5;203m";
const DEBUG: &str = "\x1b[38;5;177m";
const OK_STATUS: &str = "\x1b[38;5;82m";

static TRACING_INIT: Once = Once::new();

#[derive(Clone, Copy)]
pub enum LogLevel {
    Log,
    Warn,
    Error,
    Debug,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Log => "LOG",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Debug => "DEBUG",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Self::Log => INFO,
            Self::Warn => WARN,
            Self::Error => ERROR,
            Self::Debug => DEBUG,
        }
    }
}

pub fn log(context: &str, level: LogLevel, message: impl AsRef<str>, elapsed: Option<Duration>) {
    init_tracing();

    let pid = process::id();
    let timestamp = Local::now().format("%m/%d/%Y, %H:%M:%S");
    let level_label = format!("{:>5}", level.label());
    let elapsed = elapsed
        .map(|duration| format!(" +{}ms", duration.as_millis()))
        .unwrap_or_default();

    let line = format!(
        "{}[Caelix]{} {}{}{}  - {}{}{} {}{}{} {}[{}]{} {}{}",
        BRAND,
        RESET,
        PID,
        pid,
        RESET,
        TIME,
        timestamp,
        RESET,
        level.color(),
        level_label,
        RESET,
        CONTEXT,
        context,
        RESET,
        message.as_ref(),
        elapsed
    );

    match level {
        LogLevel::Log => tracing::info!(target: "caelix", message = line.as_str()),
        LogLevel::Warn => tracing::warn!(target: "caelix", message = line.as_str()),
        LogLevel::Error => tracing::error!(target: "caelix", message = line.as_str()),
        LogLevel::Debug => tracing::debug!(target: "caelix", message = line.as_str()),
    }
}

fn init_tracing() {
    TRACING_INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_level(false)
            .with_target(false)
            .with_ansi(false)
            .event_format(CaelixFormatter)
            .finish();

        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

struct CaelixFormatter;

impl<S, N> FormatEvent<S, N> for CaelixFormatter
where
    S: Subscriber + for<'span> LookupSpan<'span>,
    N: for<'writer> FormatFields<'writer> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        if event.metadata().target() != "caelix" {
            return Ok(());
        }

        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        writeln!(writer, "{}", visitor.message)
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl Visit for MessageVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        if field.name() == "message" && self.message.is_empty() {
            self.message = format!("{value:?}");
        }
    }
}

pub fn log_application_starting() {
    log(
        "Application",
        LogLevel::Log,
        "Starting Caelix application...",
        None,
    );
}

pub fn log_application_started(elapsed: Duration) {
    log(
        "Application",
        LogLevel::Log,
        "Caelix application successfully started",
        Some(elapsed),
    );
}

pub fn log_listening(addr: &str) {
    log(
        "Application",
        LogLevel::Log,
        format!("Caelix application listening on {}", addr),
        None,
    );
}

pub fn log_module_initialized(module: &str, elapsed: Duration) {
    log(
        "InstanceLoader",
        LogLevel::Log,
        format!("{} dependencies initialized", short_type_name(module)),
        Some(elapsed),
    );
}

pub fn log_provider_initialized(provider: &str, elapsed: Duration) {
    log(
        "ProviderLoader",
        LogLevel::Log,
        format!("{} initialized", short_type_name(provider)),
        Some(elapsed),
    );
}

pub fn log_controller_routes<C: Controller>() {
    let routes = C::routes();

    if routes.is_empty() {
        return;
    }

    log(
        "RoutesResolver",
        LogLevel::Log,
        format!(
            "{} {{{}}}:",
            short_type_name(std::any::type_name::<C>()),
            C::base_path()
        ),
        Some(Duration::ZERO),
    );

    for route in routes {
        log_route_mapped(route);
    }
}

pub fn log_route_mapped(route: &RouteDef) {
    log(
        "RouterExplorer",
        LogLevel::Log,
        format!(
            "Mapped {{{}, {}}} route",
            route.path,
            route.method.to_uppercase()
        ),
        Some(Duration::ZERO),
    );
}

pub fn log_http_request(method: &str, path: &str, status: u16, elapsed: Duration) {
    let level = match status {
        500..=599 => LogLevel::Error,
        400..=499 => LogLevel::Warn,
        _ => LogLevel::Log,
    };
    let status = color_status(status);

    log(
        "HTTP",
        level,
        format!("{} {} {}", method, path, status),
        Some(elapsed),
    );
}

pub fn log_module_routes<M: Module>() {
    let metadata = M::register();

    for import in &metadata.imports {
        (import.route_log_fn)();
    }

    for controller in &metadata.controllers {
        (controller.route_log_fn)();
    }
}

fn short_type_name(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name)
}

fn color_status(status: u16) -> String {
    let color = match status {
        500..=599 => ERROR,
        400..=499 => WARN,
        _ => OK_STATUS,
    };

    format!("{}{}{}", color, status, RESET)
}
