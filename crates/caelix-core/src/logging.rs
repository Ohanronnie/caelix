use std::{
    env, fmt,
    io::{self, BufWriter, Write},
    panic::{self, PanicHookInfo},
    process,
    sync::{
        Arc, Once, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use crossbeam_channel::{Receiver, Sender, TrySendError};
use tracing::{
    Event, Level, Metadata, Subscriber,
    field::{Field, Visit},
};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, MakeWriter, format::Writer},
    registry::LookupSpan,
};

use crate::{Controller, HttpException, Module, RouteDef};

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
const ACCESS_LOG_QUEUE_CAPACITY: usize = 65_536;
const ACCESS_LOG_BATCH_SIZE: usize = 1_024;
const ACCESS_LOG_FLUSH_INTERVAL: Duration = Duration::from_millis(100);
const ACCESS_LOG_DROP_REPORT_INTERVAL: Duration = Duration::from_secs(1);

static TRACING_INIT: Once = Once::new();
static PANIC_HOOK_INIT: Once = Once::new();
static LOG_PRIORITY: OnceLock<u8> = OnceLock::new();
static HTTP_REQUEST_LOGGING: OnceLock<bool> = OnceLock::new();
static CAELIX_TRACING_SUBSCRIBER: AtomicBool = AtomicBool::new(false);
static ACCESS_LOG_WRITER: OnceLock<AccessLogWriter> = OnceLock::new();

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

        let line = format_log_line(&self.context, level, message.as_ref(), elapsed);

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

fn format_log_line(
    context: &str,
    level: LogLevel,
    message: &str,
    elapsed: Option<Duration>,
) -> String {
    let pid = process::id();
    let timestamp = current_timestamp();
    let level_label = format!("{:>5}", level.label());
    let elapsed = elapsed
        .map(|duration| format!(" {}", format_elapsed(duration)))
        .unwrap_or_default();
    let message = match level {
        LogLevel::Error => format!("{}{}{}", ERROR, message, RESET),
        _ => message.to_string(),
    };

    format!(
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
        message,
        elapsed
    )
}

fn format_elapsed(duration: Duration) -> String {
    let milliseconds = duration.as_millis();

    if milliseconds > 0 {
        format!("+{milliseconds}ms")
    } else {
        format!("+{}µs", duration.as_micros())
    }
}

fn current_timestamp() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let total_seconds = duration.as_secs();
    let days = (total_seconds / 86_400) as i64;
    let seconds_of_day = total_seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let hour_24 = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    let period = if hour_24 < 12 { "AM" } else { "PM" };
    let hour_12 = match hour_24 % 12 {
        0 => 12,
        hour => hour,
    };

    format!("{month:02}/{day:02}/{year:04}, {hour_12:02}:{minute:02}:{second:02} {period}")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + if month <= 2 { 1 } else { 0 };

    (year, month as u32, day as u32)
}

impl crate::Injectable for Logger {
    fn create(_container: &crate::Container) -> crate::BoxFuture<'_, crate::Result<Self>> {
        Box::pin(async move { Ok(Self::new("Application")) })
    }
}

pub fn log(context: &str, level: LogLevel, message: impl AsRef<str>, elapsed: Option<Duration>) {
    Logger::new(context).write(level, message, elapsed);
}

pub fn log_http_exception(exception: &HttpException) {
    if !exception.status.is_server_error() {
        return;
    }

    let message = match &exception.source {
        Some(source) => format!(
            "{} {}: {} | source: {source:#}",
            exception.status.as_u16(),
            exception.error,
            exception.message
        ),
        None => format!(
            "{} {}: {}",
            exception.status.as_u16(),
            exception.error,
            exception.message
        ),
    };

    Logger::new("ExceptionHandler").error(message);
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

        if tracing::subscriber::set_global_default(subscriber).is_ok() {
            CAELIX_TRACING_SUBSCRIBER.store(true, Ordering::Release);
        }
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
    if !log_level_enabled(level) {
        return;
    }

    init_logging();

    if CAELIX_TRACING_SUBSCRIBER.load(Ordering::Acquire) {
        access_log_writer().write(method, path, status, elapsed);
    } else {
        let status = color_status(status);
        log(
            "HTTP",
            level,
            format!("{} {} {}", method, path, status),
            Some(elapsed),
        );
    }
}

/// Records an Actix-compatible detailed HTTP access log entry.
///
/// This is used by the Actix runtime for `Logging::info()`.
#[doc(hidden)]
#[allow(clippy::too_many_arguments)]
pub fn log_http_request_info(
    client_address: &str,
    method: &str,
    path_and_query: &str,
    protocol: &str,
    status: u16,
    response_size: Option<u64>,
    referrer: &str,
    user_agent: &str,
    elapsed: Duration,
) {
    let level = match status {
        500..=599 => LogLevel::Error,
        _ => LogLevel::Info,
    };
    if !log_level_enabled(level) {
        return;
    }

    init_logging();

    if CAELIX_TRACING_SUBSCRIBER.load(Ordering::Acquire) {
        access_log_writer().write_info(HttpAccessLogInfo::new(
            client_address,
            method,
            path_and_query,
            protocol,
            status,
            response_size,
            referrer,
            user_agent,
            elapsed,
        ));
    } else {
        log(
            "HTTP",
            level,
            format_http_request_info(
                client_address,
                method,
                path_and_query,
                protocol,
                status,
                response_size,
                referrer,
                user_agent,
                elapsed,
            ),
            None,
        );
    }
}

/// Returns the number of enabled HTTP access-log entries dropped because the
/// asynchronous writer queue was full.
pub fn dropped_http_request_logs() -> u64 {
    ACCESS_LOG_WRITER
        .get()
        .map(|writer| writer.dropped.load(Ordering::Relaxed))
        .unwrap_or(0)
}

struct AccessLogWriter {
    sender: Sender<AccessLogEvent>,
    dropped: Arc<AtomicU64>,
    pending_drop_report: Arc<AtomicU64>,
}

enum AccessLogEvent {
    Compact {
        request: String,
        status: u16,
        elapsed: Duration,
    },
    Info(HttpAccessLogInfo),
}

struct HttpAccessLogInfo {
    client_address: String,
    method: String,
    path_and_query: String,
    protocol: String,
    status: u16,
    response_size: Option<u64>,
    referrer: String,
    user_agent: String,
    elapsed: Duration,
}

impl HttpAccessLogInfo {
    #[allow(clippy::too_many_arguments)]
    fn new(
        client_address: &str,
        method: &str,
        path_and_query: &str,
        protocol: &str,
        status: u16,
        response_size: Option<u64>,
        referrer: &str,
        user_agent: &str,
        elapsed: Duration,
    ) -> Self {
        Self {
            client_address: client_address.to_string(),
            method: method.to_string(),
            path_and_query: path_and_query.to_string(),
            protocol: protocol.to_string(),
            status,
            response_size,
            referrer: referrer.to_string(),
            user_agent: user_agent.to_string(),
            elapsed,
        }
    }
}

impl AccessLogWriter {
    fn new() -> Self {
        let (sender, receiver) = crossbeam_channel::bounded(ACCESS_LOG_QUEUE_CAPACITY);
        let dropped = Arc::new(AtomicU64::new(0));
        let pending_drop_report = Arc::new(AtomicU64::new(0));
        let worker_drop_report = Arc::clone(&pending_drop_report);

        thread::Builder::new()
            .name("caelix-access-log".to_string())
            .spawn(move || write_access_logs(receiver, worker_drop_report))
            .expect("failed to start the Caelix access-log writer");

        Self {
            sender,
            dropped,
            pending_drop_report,
        }
    }

    fn write(&self, method: &str, path: &str, status: u16, elapsed: Duration) {
        if self.sender.is_full() {
            self.record_drop();
            return;
        }

        let mut request = String::with_capacity(method.len() + path.len() + 1);
        request.push_str(method);
        request.push(' ');
        request.push_str(path);

        let event = AccessLogEvent::Compact {
            request,
            status,
            elapsed,
        };

        if matches!(self.sender.try_send(event), Err(TrySendError::Full(_))) {
            self.record_drop();
        }
    }

    fn write_info(&self, event: HttpAccessLogInfo) {
        if self.sender.is_full() {
            self.record_drop();
            return;
        }

        if matches!(
            self.sender.try_send(AccessLogEvent::Info(event)),
            Err(TrySendError::Full(_))
        ) {
            self.record_drop();
        }
    }

    fn record_drop(&self) {
        self.dropped.fetch_add(1, Ordering::Relaxed);
        self.pending_drop_report.fetch_add(1, Ordering::Relaxed);
    }
}

fn access_log_writer() -> &'static AccessLogWriter {
    ACCESS_LOG_WRITER.get_or_init(AccessLogWriter::new)
}

fn write_access_logs(receiver: Receiver<AccessLogEvent>, dropped: Arc<AtomicU64>) {
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut stdout = BufWriter::with_capacity(256 * 1024, stdout.lock());
    let mut stderr = BufWriter::with_capacity(16 * 1024, stderr.lock());
    let mut last_drop_report = Instant::now();

    loop {
        match receiver.recv_timeout(ACCESS_LOG_FLUSH_INTERVAL) {
            Ok(event) => {
                write_access_log(&mut stdout, &mut stderr, event);

                for event in receiver.try_iter().take(ACCESS_LOG_BATCH_SIZE - 1) {
                    write_access_log(&mut stdout, &mut stderr, event);
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }

        if last_drop_report.elapsed() >= ACCESS_LOG_DROP_REPORT_INTERVAL {
            report_dropped_access_logs(&mut stderr, &dropped);
            last_drop_report = Instant::now();
        }

        let _ = stdout.flush();
        let _ = stderr.flush();
    }

    report_dropped_access_logs(&mut stderr, &dropped);
    let _ = stdout.flush();
    let _ = stderr.flush();
}

fn write_access_log(
    stdout: &mut BufWriter<io::StdoutLock<'_>>,
    stderr: &mut BufWriter<io::StderrLock<'_>>,
    event: AccessLogEvent,
) {
    let (level, message, elapsed) = match event {
        AccessLogEvent::Compact {
            request,
            status,
            elapsed,
        } => {
            let level = log_level_for_status(status);
            (
                level,
                format!("{} {}", request, color_status(status)),
                Some(elapsed),
            )
        }
        AccessLogEvent::Info(info) => {
            let level = log_level_for_status(info.status);
            (
                level,
                format_http_request_info(
                    &info.client_address,
                    &info.method,
                    &info.path_and_query,
                    &info.protocol,
                    info.status,
                    info.response_size,
                    &info.referrer,
                    &info.user_agent,
                    info.elapsed,
                ),
                None,
            )
        }
    };
    let line = format_log_line("HTTP", level, &message, elapsed);

    if level == LogLevel::Error {
        let _ = writeln!(stderr, "{line}");
    } else {
        let _ = writeln!(stdout, "{line}");
    }
}

fn log_level_for_status(status: u16) -> LogLevel {
    match status {
        500..=599 => LogLevel::Error,
        _ => LogLevel::Info,
    }
}

#[allow(clippy::too_many_arguments)]
fn format_http_request_info(
    client_address: &str,
    method: &str,
    path_and_query: &str,
    protocol: &str,
    status: u16,
    response_size: Option<u64>,
    referrer: &str,
    user_agent: &str,
    elapsed: Duration,
) -> String {
    let request = if path_and_query.is_empty() {
        method.to_string()
    } else {
        format!("{method} {path_and_query}")
    };
    let response_size = response_size
        .map(|size| size.to_string())
        .unwrap_or_else(|| "-".to_string());

    format!(
        "{client_address} \"{request} {protocol}\" {} {response_size} \"{referrer}\" \"{user_agent}\" {:.6}",
        color_status(status),
        elapsed.as_secs_f64(),
    )
}

fn report_dropped_access_logs(stderr: &mut BufWriter<io::StderrLock<'_>>, dropped: &AtomicU64) {
    let dropped = dropped.swap(0, Ordering::Relaxed);

    if dropped > 0 {
        let _ = writeln!(
            stderr,
            "[Caelix] access-log writer queue was full; dropped {dropped} entries"
        );
    }
}

pub fn http_request_logging_enabled() -> bool {
    *HTTP_REQUEST_LOGGING.get_or_init(|| {
        env::var("CAELIX_HTTP_LOG")
            .or_else(|_| env::var("CAELIX_ACCESS_LOG"))
            .ok()
            .and_then(|value| parse_bool(&value))
            .unwrap_or(false)
    })
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
    *LOG_PRIORITY.get_or_init(|| {
        env::var("CAELIX_LOG")
            .ok()
            .and_then(|value| parse_log_level(&value))
            .or_else(|| {
                env::var("RUST_LOG")
                    .ok()
                    .and_then(|value| parse_rust_log_level(&value))
            })
            .unwrap_or(LogLevel::Info.priority())
    })
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

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "y" | "on" => Some(true),
        "0" | "false" | "no" | "n" | "off" => Some(false),
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

    #[test]
    fn parse_bool_accepts_common_env_values() {
        assert_eq!(parse_bool("1"), Some(true));
        assert_eq!(parse_bool("on"), Some(true));
        assert_eq!(parse_bool("false"), Some(false));
        assert_eq!(parse_bool("off"), Some(false));
        assert_eq!(parse_bool("sometimes"), None);
    }

    #[test]
    fn detailed_http_log_includes_actix_default_fields() {
        let line = format_http_request_info(
            "127.0.0.1:43120",
            "GET",
            "/hello?name=Caelix",
            "HTTP/1.1",
            200,
            Some(42),
            "https://example.com",
            "CaelixTest/1.0",
            Duration::from_micros(1_250),
        );

        assert!(line.starts_with("127.0.0.1:43120 \"GET /hello?name=Caelix HTTP/1.1\""));
        assert!(line.contains(" 42 \"https://example.com\" \"CaelixTest/1.0\" 0.001250"));
    }
}
