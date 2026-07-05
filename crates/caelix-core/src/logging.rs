use std::{
    env, fmt,
    io::{self, Write},
    panic::{self, PanicHookInfo},
    process,
    sync::Once,
    time::Duration,
};

use chrono::Local;
use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, MakeWriter, format::Writer},
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
static PANIC_HOOK_INIT: Once = Once::new();

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Info,
    Log,
    Warn,
    Error,
    Debug,
}

impl LogLevel {
    fn label(self) -> &'static str {
        match self {
            Self::Info | Self::Log => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Debug => "DEBUG",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Self::Info | Self::Log => INFO,
            Self::Warn => WARN,
            Self::Error => ERROR,
            Self::Debug => DEBUG,
        }
    }

    fn priority(self) -> u8 {
        match self {
            Self::Error => 1,
            Self::Warn => 2,
            Self::Info | Self::Log => 3,
            Self::Debug => 4,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Logger {
    context: String,
}

impl Logger {
    pub fn new(context: impl Into<String>) -> Self {
        Self {
            context: short_type_name(&context.into()).to_string(),
        }
    }

    pub fn for_type<T: ?Sized>() -> Self {
        Self::new(std::any::type_name::<T>())
    }

    pub fn context(&self) -> &str {
        &self.context
    }

    pub fn info(&self, message: impl AsRef<str>) {
        self.write(LogLevel::Info, message, None);
    }

    pub fn warn(&self, message: impl AsRef<str>) {
        self.write(LogLevel::Warn, message, None);
    }

    pub fn error(&self, message: impl AsRef<str>) {
        self.write(LogLevel::Error, message, None);
    }

    pub fn debug(&self, message: impl AsRef<str>) {
        self.write(LogLevel::Debug, message, None);
    }

    pub(crate) fn write(
        &self,
        level: LogLevel,
        message: impl AsRef<str>,
        elapsed: Option<Duration>,
    ) {
        self.write_inner(level, message, elapsed, false);
    }

    fn write_forced(&self, level: LogLevel, message: impl AsRef<str>) {
        self.write_inner(level, message, None, true);
    }

    fn write_inner(
        &self,
        level: LogLevel,
        message: impl AsRef<str>,
        elapsed: Option<Duration>,
        force: bool,
    ) {
        if !force && !log_level_enabled(level) {
            return;
        }

        init_logging();

        let pid = process::id();
        let timestamp = Local::now().format("%m/%d/%Y, %I:%M:%S %p");
        let level_label = format!("{:>5}", level.label());
        let elapsed = elapsed
            .map(|duration| format!(" {}", format_elapsed(duration)))
            .unwrap_or_default();

        let message = match level {
            LogLevel::Error => format!("{}{}{}", ERROR, message.as_ref(), RESET),
            _ => message.as_ref().to_string(),
        };

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
            self.context,
            RESET,
            message,
            elapsed
        );

        match level {
            LogLevel::Info | LogLevel::Log => {
                tracing::info!(target: "caelix", message = line.as_str())
            }
            LogLevel::Warn => tracing::warn!(target: "caelix", message = line.as_str()),
            LogLevel::Error => tracing::error!(target: "caelix", message = line.as_str()),
            LogLevel::Debug => tracing::debug!(target: "caelix", message = line.as_str()),
        }
    }
}

fn format_elapsed(duration: Duration) -> String {
    let milliseconds = duration.as_millis();

    if milliseconds > 0 {
        format!("+{milliseconds}ms")
    } else {
        format!("+{}µs", duration.as_micros())
    }
}

impl crate::Injectable for Logger {
    fn create(_container: &crate::Container) -> crate::BoxFuture<'_, Self> {
        Box::pin(async move { Self::new("Application") })
    }
}

pub fn log(context: &str, level: LogLevel, message: impl AsRef<str>, elapsed: Option<Duration>) {
    Logger::new(context).write(level, message, elapsed);
}

pub(crate) fn init_logging() {
    install_panic_hook();
    init_tracing();
}

fn init_tracing() {
    TRACING_INIT.call_once(|| {
        let subscriber = tracing_subscriber::fmt()
            .without_time()
            .with_level(false)
            .with_target(false)
            .with_ansi(false)
            .with_writer(CaelixMakeWriter)
            .event_format(CaelixFormatter)
            .finish();

        let _ = tracing::subscriber::set_global_default(subscriber);
    });
}

fn install_panic_hook() {
    PANIC_HOOK_INIT.call_once(|| {
        panic::set_hook(Box::new(|info| {
            Logger::new("ExceptionHandler").write_forced(LogLevel::Error, panic_message(info));
        }));
    });
}

fn panic_message(info: &PanicHookInfo<'_>) -> String {
    let payload = if let Some(message) = info.payload().downcast_ref::<&str>() {
        *message
    } else if let Some(message) = info.payload().downcast_ref::<String>() {
        message.as_str()
    } else {
        "panic occurred"
    };

    if let Some(location) = info.location() {
        format!(
            "panic: {} (at {}:{}:{})",
            payload,
            location.file(),
            location.line(),
            location.column()
        )
    } else {
        format!("panic: {}", payload)
    }
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

struct CaelixMakeWriter;

impl<'a> MakeWriter<'a> for CaelixMakeWriter {
    type Writer = CaelixStreamWriter;

    fn make_writer(&'a self) -> Self::Writer {
        CaelixStreamWriter::Stdout(io::stdout())
    }

    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        if *meta.level() == Level::ERROR {
            CaelixStreamWriter::Stderr(io::stderr())
        } else {
            CaelixStreamWriter::Stdout(io::stdout())
        }
    }
}

enum CaelixStreamWriter {
    Stdout(io::Stdout),
    Stderr(io::Stderr),
}

impl Write for CaelixStreamWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Stdout(writer) => writer.write(buf),
            Self::Stderr(writer) => writer.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Stdout(writer) => writer.flush(),
            Self::Stderr(writer) => writer.flush(),
        }
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
        LogLevel::Info,
        "Starting Caelix application...",
        None,
    );
}

pub fn log_application_started(elapsed: Duration) {
    log(
        "Application",
        LogLevel::Info,
        "Caelix application successfully started",
        Some(elapsed),
    );
}

pub fn log_listening(addr: &str) {
    log(
        "Application",
        LogLevel::Info,
        format!("Caelix application listening on {}", addr),
        None,
    );
}

pub fn log_module_initialized(module: &str, elapsed: Duration) {
    log(
        "InstanceLoader",
        LogLevel::Info,
        format!("{} dependencies initialized", short_type_name(module)),
        Some(elapsed),
    );
}

pub fn log_provider_initialized(provider: &str, elapsed: Duration) {
    log(
        "ProviderLoader",
        LogLevel::Info,
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
        LogLevel::Info,
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
        LogLevel::Info,
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
        _ => LogLevel::Info,
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

fn log_level_enabled(level: LogLevel) -> bool {
    level.priority() <= configured_log_priority()
}

fn configured_log_priority() -> u8 {
    env::var("CAELIX_LOG")
        .ok()
        .and_then(|value| parse_log_level(&value))
        .or_else(|| {
            env::var("RUST_LOG")
                .ok()
                .and_then(|value| parse_rust_log_level(&value))
        })
        .unwrap_or(LogLevel::Info.priority())
}

fn parse_rust_log_level(value: &str) -> Option<u8> {
    value.split(',').find_map(|directive| {
        let directive = directive.trim();

        if let Some((target, level)) = directive.split_once('=') {
            let target = target.trim();
            if matches!(target, "caelix" | "caelix_core" | "caelix-core") {
                return parse_log_level(level);
            }

            return None;
        }

        parse_log_level(directive)
    })
}

fn parse_log_level(value: &str) -> Option<u8> {
    match value.trim().to_ascii_lowercase().as_str() {
        "debug" | "trace" => Some(LogLevel::Debug.priority()),
        "info" | "log" => Some(LogLevel::Info.priority()),
        "warn" | "warning" => Some(LogLevel::Warn.priority()),
        "error" => Some(LogLevel::Error.priority()),
        "off" => Some(0),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_format_preserves_sub_millisecond_detail() {
        assert_eq!(format_elapsed(Duration::ZERO), "+0µs");
        assert_eq!(format_elapsed(Duration::from_micros(742)), "+742µs");
        assert_eq!(format_elapsed(Duration::from_micros(1_200)), "+1ms");
        assert_eq!(format_elapsed(Duration::from_millis(42)), "+42ms");
    }
}
