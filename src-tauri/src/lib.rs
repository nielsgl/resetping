use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    process::Command,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use rand::RngExt as _;
use reqwest::Client;
use serde::{Deserialize, Serialize};
#[cfg(target_os = "macos")]
use tauri::ActivationPolicy;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Emitter, Manager, State,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt as _;
use tokio::time::sleep;

const DEFAULT_ENDPOINT: &str = "https://hascodexratelimitreset.today/api/status";
const DEFAULT_UPDATER_ENDPOINT: &str =
    "https://github.com/nielsgl/resetping/releases/latest/download/latest.json";
const APP_USER_AGENT: &str = "ResetPing/0.1.0 (+https://github.com/nielsgl/resetping)";
const STATE_FILE: &str = "state.json";
const TRANSITIONS_LIMIT: usize = 100;
const LOG_LIMIT: usize = 200;
const FAILURE_THRESHOLD: u32 = 5;
#[cfg(debug_assertions)]
const HEARTBEAT_INTERVAL_MS: u64 = 10 * 60 * 1000;
#[cfg(not(debug_assertions))]
const HEARTBEAT_INTERVAL_MS: u64 = 24 * 60 * 60 * 1000;
static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();

#[cfg(all(feature = "http-native-tls", feature = "http-rustls"))]
compile_error!("Enable only one HTTP backend feature: `http-native-tls` or `http-rustls`.");

#[cfg(not(any(feature = "http-native-tls", feature = "http-rustls")))]
compile_error!("One HTTP backend feature is required: `http-native-tls` or `http-rustls`.");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
enum NotificationPolicy {
    #[default]
    Flip,
    NoToYes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AppSettings {
    #[serde(default = "default_poll_interval_sec")]
    poll_interval_sec: u64,
    #[serde(default = "default_low_power_poll_interval_sec")]
    low_power_poll_interval_sec: u64,
    #[serde(default = "default_http_timeout_ms")]
    http_timeout_ms: u64,
    #[serde(default)]
    notification_policy: NotificationPolicy,
    #[serde(default = "default_true")]
    notify_initial_state: bool,
    #[serde(default)]
    launch_at_login: bool,
    #[serde(default)]
    update_checks_enabled: bool,
    #[serde(default = "default_update_check_interval_hours")]
    update_check_interval_hours: u64,
    #[serde(default = "default_endpoint_url")]
    status_endpoint_url: String,
    #[serde(default = "default_true")]
    error_telemetry_enabled: bool,
    #[serde(default)]
    usage_telemetry_enabled: bool,
    #[serde(default, rename = "telemetry_enabled", skip_serializing)]
    legacy_telemetry_enabled: Option<bool>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            poll_interval_sec: default_poll_interval_sec(),
            low_power_poll_interval_sec: default_low_power_poll_interval_sec(),
            http_timeout_ms: default_http_timeout_ms(),
            notification_policy: NotificationPolicy::Flip,
            notify_initial_state: true,
            launch_at_login: false,
            update_checks_enabled: false,
            update_check_interval_hours: default_update_check_interval_hours(),
            status_endpoint_url: default_endpoint_url(),
            error_telemetry_enabled: true,
            usage_telemetry_enabled: false,
            legacy_telemetry_enabled: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RuntimeSnapshot {
    last_known_state: Option<String>,
    last_success_at: Option<u64>,
    consecutive_failures: u32,
    last_error_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TransitionEntry {
    from: String,
    to: String,
    detected_at: u64,
    source_updated_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
struct LogEntry {
    timestamp_ms: u64,
    level: String,
    message: String,
}

#[derive(Debug, Clone, Serialize)]
struct AppStateResponse {
    settings: AppSettings,
    snapshot: RuntimeSnapshot,
    transitions: Vec<TransitionEntry>,
    logs: Vec<LogEntry>,
    health: String,
    installation_id: String,
    frontend_telemetry_dsn: Option<String>,
    updater: Option<UpdateCheckResponse>,
}

#[derive(Debug, Clone, Serialize)]
struct UpdateCheckResponse {
    checked_at: u64,
    status: UpdateCheckStatus,
    version: Option<String>,
    current_version: Option<String>,
    notes: Option<String>,
    install_ready: bool,
    message: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum UpdateCheckStatus {
    UpToDate,
    UpdateAvailable,
    UnsupportedPlatform,
    CheckFailed,
}

#[derive(Debug, Clone, Copy)]
enum UpdateCheckTrigger {
    Manual,
    Scheduled,
}

#[derive(Debug, Clone, Deserialize)]
struct StatusApiPayload {
    state: String,
    configured: bool,
    #[serde(rename = "updatedAt")]
    updated_at: Option<u64>,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct NormalizedStatus {
    effective_state: String,
    source_timestamp_ms: Option<u64>,
    fetched_at_ms: u64,
    configured: bool,
    transport: FetchTransport,
}

#[derive(Debug, Clone, Copy)]
enum FetchTransport {
    Reqwest,
    CurlFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistentData {
    settings: AppSettings,
    snapshot: RuntimeSnapshot,
    transitions: Vec<TransitionEntry>,
    installation_id: Option<String>,
    last_usage_heartbeat_at_ms: Option<u64>,
}

struct RuntimeState {
    settings: AppSettings,
    snapshot: RuntimeSnapshot,
    transitions: VecDeque<TransitionEntry>,
    logs: VecDeque<LogEntry>,
    in_flight: bool,
    last_update_check_ms: Option<u64>,
    pending_update: Option<tauri_plugin_updater::Update>,
    updater: Option<UpdateCheckResponse>,
    background_notified_update_version: Option<String>,
    installation_id: String,
    last_usage_heartbeat_at_ms: Option<u64>,
    sentry_dsn: Option<String>,
}

#[derive(Clone)]
struct SharedState {
    inner: Arc<Mutex<RuntimeState>>,
}

fn default_poll_interval_sec() -> u64 {
    60
}

fn default_low_power_poll_interval_sec() -> u64 {
    300
}

fn default_http_timeout_ms() -> u64 {
    8_000
}

fn default_update_check_interval_hours() -> u64 {
    24
}

fn default_endpoint_url() -> String {
    DEFAULT_ENDPOINT.to_string()
}

fn default_true() -> bool {
    true
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn build_channel() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

fn http_backend_label() -> &'static str {
    #[cfg(feature = "http-native-tls")]
    {
        "native-tls"
    }
    #[cfg(all(not(feature = "http-native-tls"), feature = "http-rustls"))]
    {
        "rustls"
    }
}

fn default_installation_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn telemetry_enabled(settings: &AppSettings) -> bool {
    settings.error_telemetry_enabled || settings.usage_telemetry_enabled
}

fn heartbeat_due(last_usage_heartbeat_at_ms: Option<u64>, now: u64) -> bool {
    match last_usage_heartbeat_at_ms {
        None => true,
        Some(last) => now.saturating_sub(last) >= HEARTBEAT_INTERVAL_MS,
    }
}

fn should_emit_poll_failure_telemetry(consecutive_failures: u32) -> bool {
    matches!(consecutive_failures, 1 | 5 | 20 | 100)
}

fn updater_public_key() -> Option<&'static str> {
    option_env!("TAURI_UPDATER_PUBLIC_KEY").filter(|value| !value.trim().is_empty())
}

fn updater_supported_platform() -> bool {
    cfg!(target_os = "macos")
}

fn update_check_due(last_check: Option<u64>, now: u64, interval_hours: u64) -> bool {
    let due_ms = interval_hours.saturating_mul(60 * 60 * 1000);
    last_check
        .map(|last| now.saturating_sub(last) >= due_ms)
        .unwrap_or(true)
}

fn update_response(
    checked_at: u64,
    status: UpdateCheckStatus,
    version: Option<String>,
    current_version: Option<String>,
    notes: Option<String>,
    install_ready: bool,
    message: impl Into<String>,
) -> UpdateCheckResponse {
    UpdateCheckResponse {
        checked_at,
        status,
        version,
        current_version,
        notes,
        install_ready,
        message: message.into(),
    }
}

fn sanitize_settings(mut settings: AppSettings) -> AppSettings {
    if let Some(legacy) = settings.legacy_telemetry_enabled.take() {
        settings.error_telemetry_enabled = legacy;
        settings.usage_telemetry_enabled = legacy;
    }

    settings.poll_interval_sec = settings.poll_interval_sec.clamp(30, 900);
    settings.low_power_poll_interval_sec = settings.low_power_poll_interval_sec.clamp(60, 900);
    settings.http_timeout_ms = settings.http_timeout_ms.clamp(1_000, 30_000);
    settings.update_check_interval_hours = settings.update_check_interval_hours.clamp(1, 24 * 30);

    if settings.status_endpoint_url.trim().is_empty() {
        settings.status_endpoint_url = default_endpoint_url();
    }

    settings
}

fn push_log(state: &mut RuntimeState, level: &str, message: impl Into<String>) {
    state.logs.push_back(LogEntry {
        timestamp_ms: now_ms(),
        level: level.to_string(),
        message: message.into(),
    });

    while state.logs.len() > LOG_LIMIT {
        state.logs.pop_front();
    }
}

fn is_degraded(snapshot: &RuntimeSnapshot) -> bool {
    snapshot.consecutive_failures >= FAILURE_THRESHOLD
}

fn health_label(snapshot: &RuntimeSnapshot) -> String {
    if is_degraded(snapshot) {
        "degraded".to_string()
    } else {
        "healthy".to_string()
    }
}

fn state_path(app: &AppHandle) -> Result<PathBuf, String> {
    let mut path = app
        .path()
        .app_config_dir()
        .map_err(|e| format!("failed to resolve config dir: {e}"))?;
    fs::create_dir_all(&path).map_err(|e| format!("failed to create config dir: {e}"))?;
    path.push(STATE_FILE);
    Ok(path)
}

fn load_persistent(app: &AppHandle) -> PersistentData {
    let Ok(path) = state_path(app) else {
        return PersistentData::default();
    };

    let Ok(contents) = fs::read_to_string(path) else {
        return PersistentData::default();
    };

    serde_json::from_str::<PersistentData>(&contents).unwrap_or_default()
}

fn save_persistent(app: &AppHandle, state: &RuntimeState) -> Result<(), String> {
    let path = state_path(app)?;
    let payload = PersistentData {
        settings: state.settings.clone(),
        snapshot: state.snapshot.clone(),
        transitions: state.transitions.iter().cloned().collect(),
        installation_id: Some(state.installation_id.clone()),
        last_usage_heartbeat_at_ms: state.last_usage_heartbeat_at_ms,
    };

    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| format!("failed to serialize state: {e}"))?;
    fs::write(path, json).map_err(|e| format!("failed to write state file: {e}"))
}

fn emit_state_changed(app: &AppHandle, runtime: &RuntimeState) {
    let _ = app.emit(
        "state-updated",
        AppStateResponse {
            settings: runtime.settings.clone(),
            snapshot: runtime.snapshot.clone(),
            transitions: runtime.transitions.iter().cloned().collect(),
            logs: runtime.logs.iter().cloned().collect(),
            health: health_label(&runtime.snapshot),
            installation_id: runtime.installation_id.clone(),
            frontend_telemetry_dsn: runtime.sentry_dsn.clone(),
            updater: runtime.updater.clone(),
        },
    );
}

fn update_tray_tooltip(app: &AppHandle, snapshot: &RuntimeSnapshot) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };

    let status = snapshot
        .last_known_state
        .as_deref()
        .unwrap_or("unknown")
        .to_uppercase();
    let checked = snapshot
        .last_success_at
        .map(|v| v.to_string())
        .unwrap_or_else(|| "never".to_string());

    let tooltip = format!(
        "ResetPing\nStatus: {status}\nLast success: {checked}\nHealth: {}",
        health_label(snapshot)
    );

    let _ = tray.set_tooltip(Some(tooltip));
}

fn set_autostart(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable()
            .map_err(|e| format!("enable autostart failed: {e}"))
    } else {
        mgr.disable()
            .map_err(|e| format!("disable autostart failed: {e}"))
    }
}

fn show_settings_window(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn send_notification(app: &AppHandle, title: &str, body: &str) {
    let _ = app.notification().builder().title(title).body(body).show();
}

fn build_status_line(snapshot: &RuntimeSnapshot) -> String {
    let state = snapshot.last_known_state.as_deref().unwrap_or("unknown");
    let health = health_label(snapshot);
    let last = snapshot
        .last_success_at
        .map(|v| v.to_string())
        .unwrap_or_else(|| "never".to_string());
    format!("State: {state} | Health: {health} | Last success: {last}")
}

fn resolve_sentry_dsn() -> Option<String> {
    let _ = dotenvy::dotenv();
    std::env::var("SENTRY_DSN").ok()
}

fn init_telemetry(enabled: bool, dsn: Option<&str>) {
    if !enabled {
        return;
    }

    let Some(dsn) = dsn else {
        return;
    };

    let guard = sentry::init((
        dsn,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            // Capture user IPs and potentially sensitive headers when using HTTP server integrations
            // see https://docs.sentry.io/platforms/rust/data-management/data-collected for more info
            // send_default_pii: true,
            ..Default::default()
        },
    ));

    let _ = SENTRY_GUARD.set(guard);
    sentry::capture_message("ResetPing telemetry initialized", sentry::Level::Info);
}

fn capture_telemetry_error(
    settings: &AppSettings,
    installation_id: &str,
    message: &str,
    component: &str,
    error_kind: &str,
) {
    if !settings.error_telemetry_enabled || SENTRY_GUARD.get().is_none() {
        return;
    }

    sentry::with_scope(
        |scope| {
            scope.set_tag("event_type", "error");
            scope.set_tag("component", component.to_string());
            scope.set_tag("error_kind", error_kind.to_string());
            scope.set_tag("platform", std::env::consts::OS.to_string());
            scope.set_tag("app_version", env!("CARGO_PKG_VERSION").to_string());
            scope.set_tag("build_channel", build_channel().to_string());
            scope.set_tag("installation_id", installation_id.to_string());
        },
        || {
            let err = std::io::Error::other(message.to_string());
            sentry::capture_error(&err)
        },
    );
}

fn capture_telemetry_message(
    settings: &AppSettings,
    installation_id: &str,
    event_type: &str,
    component: &str,
    level: sentry::Level,
    message: &str,
) -> Option<String> {
    if !settings.error_telemetry_enabled || SENTRY_GUARD.get().is_none() {
        return None;
    }

    Some(
        sentry::with_scope(
            |scope| {
                scope.set_tag("event_type", event_type.to_string());
                scope.set_tag("component", component.to_string());
                scope.set_tag("platform", std::env::consts::OS.to_string());
                scope.set_tag("app_version", env!("CARGO_PKG_VERSION").to_string());
                scope.set_tag("build_channel", build_channel().to_string());
                scope.set_tag("installation_id", installation_id.to_string());
            },
            || sentry::capture_message(message, level),
        )
        .to_string(),
    )
}

fn capture_usage_event(
    settings: &AppSettings,
    installation_id: &str,
    event_type: &str,
    component: &str,
    message: &str,
) {
    if !settings.usage_telemetry_enabled || SENTRY_GUARD.get().is_none() {
        return;
    }

    sentry::with_scope(
        |scope| {
            scope.set_tag("event_type", event_type.to_string());
            scope.set_tag("component", component.to_string());
            scope.set_tag("platform", std::env::consts::OS.to_string());
            scope.set_tag("app_version", env!("CARGO_PKG_VERSION").to_string());
            scope.set_tag("build_channel", build_channel().to_string());
            scope.set_tag("installation_id", installation_id.to_string());
        },
        || {
            sentry::capture_message(message, sentry::Level::Info);
        },
    );
}

#[cfg(target_os = "macos")]
fn low_power_mode_active() -> bool {
    let Ok(output) = Command::new("pmset").args(["-g", "batt"]).output() else {
        return false;
    };

    let text = String::from_utf8_lossy(&output.stdout).to_lowercase();
    text.contains("low power mode: 1")
}

#[cfg(not(target_os = "macos"))]
fn low_power_mode_active() -> bool {
    false
}

async fn fetch_status(settings: &AppSettings) -> Result<NormalizedStatus, String> {
    let mut builder = Client::builder().timeout(Duration::from_millis(settings.http_timeout_ms));
    #[cfg(feature = "http-native-tls")]
    {
        builder = builder.use_native_tls();
    }
    #[cfg(feature = "http-rustls")]
    {
        builder = builder.use_rustls_tls();
    }

    let client = builder
        .build()
        .map_err(|e| format!("failed to build client: {e:#} (debug: {e:?})"))?;

    let response = client
        .get(settings.status_endpoint_url.clone())
        .header("cache-control", "no-store")
        .header("user-agent", APP_USER_AGENT)
        .send()
        .await;

    let response = match response {
        Ok(resp) => resp,
        Err(err) => {
            let msg = format!("request failed: {err:#} (debug: {err:?})");
            if msg.to_lowercase().contains("bad file descriptor") {
                return fetch_status_via_curl(settings)
                    .await
                    .map_err(|fallback_err| {
                        format!("{msg}; curl fallback failed: {fallback_err}")
                    });
            }
            return Err(msg);
        }
    };

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("invalid response body: {e:#} (debug: {e:?})"))?;

    let payload: StatusApiPayload = serde_json::from_str(&body).map_err(|e| {
        if status.is_success() {
            format!("invalid response body: {e:#} (debug: {e:?})")
        } else {
            format!(
                "request failed with status {status} and unparsable body: {e:#} (debug: {e:?}); body={body}"
            )
        }
    })?;

    if !status.is_success() {
        let upstream = payload
            .error
            .as_deref()
            .unwrap_or("no upstream error message");
        return Err(format!(
            "request failed with status {status}; upstream_error={upstream}"
        ));
    }

    let effective_state = if payload.state == "no" { "no" } else { "yes" };

    Ok(NormalizedStatus {
        effective_state: effective_state.to_string(),
        source_timestamp_ms: payload.updated_at,
        fetched_at_ms: now_ms(),
        configured: payload.configured,
        transport: FetchTransport::Reqwest,
    })
}

async fn fetch_status_via_curl(settings: &AppSettings) -> Result<NormalizedStatus, String> {
    let timeout_secs = std::cmp::max(1, settings.http_timeout_ms.div_ceil(1000));
    let endpoint = settings.status_endpoint_url.clone();
    let output = tauri::async_runtime::spawn_blocking(move || {
        Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--max-time",
                &timeout_secs.to_string(),
                "--connect-timeout",
                &timeout_secs.to_string(),
                "--header",
                "cache-control: no-store",
                "--user-agent",
                APP_USER_AGENT,
                "--write-out",
                "\n__HTTP_STATUS__:%{http_code}",
                &endpoint,
            ])
            .output()
    })
    .await
    .map_err(|e| format!("curl fallback join error: {e}"))?
    .map_err(|e| format!("curl fallback execution failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "curl fallback request failed with status {}: {}",
            output.status, stderr
        ));
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let (body, status_code) = stdout
        .rsplit_once("\n__HTTP_STATUS__:")
        .ok_or_else(|| "curl fallback missing HTTP status trailer".to_string())?;
    let status_code: u16 = status_code
        .trim()
        .parse()
        .map_err(|e| format!("curl fallback invalid HTTP status trailer: {e}"))?;

    let payload: StatusApiPayload = serde_json::from_str(body)
        .map_err(|e| format!("curl fallback invalid response body: {e:#} (debug: {e:?})"))?;
    if status_code >= 400 {
        let upstream = payload
            .error
            .as_deref()
            .unwrap_or("no upstream error message");
        return Err(format!(
            "request failed with status {status_code}; upstream_error={upstream}"
        ));
    }
    let effective_state = if payload.state == "no" { "no" } else { "yes" };

    Ok(NormalizedStatus {
        effective_state: effective_state.to_string(),
        source_timestamp_ms: payload.updated_at,
        fetched_at_ms: now_ms(),
        configured: payload.configured,
        transport: FetchTransport::CurlFallback,
    })
}

async fn perform_update_check(
    app: &AppHandle,
    state: &SharedState,
    trigger: UpdateCheckTrigger,
) -> UpdateCheckResponse {
    let checked_at = now_ms();
    let (settings, installation_id) = {
        let mut guard = state.inner.lock().unwrap();
        guard.last_update_check_ms = Some(checked_at);
        capture_usage_event(
            &guard.settings,
            &guard.installation_id,
            "update_check",
            "updater",
            match trigger {
                UpdateCheckTrigger::Manual => "Manual update check requested",
                UpdateCheckTrigger::Scheduled => "Background update check requested",
            },
        );
        push_log(
            &mut guard,
            "info",
            match trigger {
                UpdateCheckTrigger::Manual => "Manual update check requested",
                UpdateCheckTrigger::Scheduled => "Background update check requested",
            },
        );
        (guard.settings.clone(), guard.installation_id.clone())
    };

    let response = if !updater_supported_platform() {
        update_response(
            checked_at,
            UpdateCheckStatus::UnsupportedPlatform,
            None,
            Some(env!("CARGO_PKG_VERSION").to_string()),
            None,
            false,
            "In-app updater is enabled on macOS only for v1.",
        )
    } else {
        match updater_public_key() {
            None => update_response(
                checked_at,
                UpdateCheckStatus::CheckFailed,
                None,
                Some(env!("CARGO_PKG_VERSION").to_string()),
                None,
                false,
                "Updater public key is not configured.",
            ),
            Some(pubkey) => {
                let endpoint = match DEFAULT_UPDATER_ENDPOINT.parse::<tauri::Url>() {
                    Ok(endpoint) => endpoint,
                    Err(err) => {
                        return update_response(
                            checked_at,
                            UpdateCheckStatus::CheckFailed,
                            None,
                            Some(env!("CARGO_PKG_VERSION").to_string()),
                            None,
                            false,
                            format!("Updater endpoint URL is invalid: {err}"),
                        )
                    }
                };
                let builder = app.updater_builder().endpoints(vec![endpoint]);
                let updater = match builder {
                    Ok(builder) => builder.pubkey(pubkey).build(),
                    Err(err) => Err(err),
                };

                match updater {
                    Ok(updater) => match updater.check().await {
                        Ok(Some(update)) => {
                            let version = update.version.clone();
                            let current_version = update.current_version.clone();
                            let notes = update.body.clone();
                            {
                                let mut guard = state.inner.lock().unwrap();
                                guard.pending_update = Some(update);
                            }
                            update_response(
                                checked_at,
                                UpdateCheckStatus::UpdateAvailable,
                                Some(version),
                                Some(current_version),
                                notes,
                                true,
                                "Update is available. Use Install Update to apply it.",
                            )
                        }
                        Ok(None) => update_response(
                            checked_at,
                            UpdateCheckStatus::UpToDate,
                            None,
                            Some(env!("CARGO_PKG_VERSION").to_string()),
                            None,
                            false,
                            "ResetPing is up to date.",
                        ),
                        Err(err) => {
                            if settings.error_telemetry_enabled {
                                capture_telemetry_error(
                                    &settings,
                                    &installation_id,
                                    &format!("Updater check failed: {err}"),
                                    "updater",
                                    "check_failed",
                                );
                            }
                            update_response(
                                checked_at,
                                UpdateCheckStatus::CheckFailed,
                                None,
                                Some(env!("CARGO_PKG_VERSION").to_string()),
                                None,
                                false,
                                format!("Update check failed: {err}"),
                            )
                        }
                    },
                    Err(err) => {
                        if settings.error_telemetry_enabled {
                            capture_telemetry_error(
                                &settings,
                                &installation_id,
                                &format!("Updater initialization failed: {err}"),
                                "updater",
                                "setup_failed",
                            );
                        }
                        update_response(
                            checked_at,
                            UpdateCheckStatus::CheckFailed,
                            None,
                            Some(env!("CARGO_PKG_VERSION").to_string()),
                            None,
                            false,
                            format!("Updater initialization failed: {err}"),
                        )
                    }
                }
            }
        }
    };

    {
        let mut guard = state.inner.lock().unwrap();
        let available_version = response.version.clone();
        let should_notify_background = matches!(trigger, UpdateCheckTrigger::Scheduled)
            && response.status == UpdateCheckStatus::UpdateAvailable
            && available_version.as_ref() != guard.background_notified_update_version.as_ref();

        if response.status != UpdateCheckStatus::UpdateAvailable {
            guard.pending_update = None;
        }

        if response.status != UpdateCheckStatus::UpdateAvailable {
            guard.background_notified_update_version = None;
        }

        if should_notify_background {
            if let Some(version) = available_version.clone() {
                send_notification(
                    app,
                    "ResetPing: update available",
                    &format!("Version {version} is available. Open Settings to install."),
                );
                guard.background_notified_update_version = Some(version);
            }
        }

        push_log(
            &mut guard,
            if response.status == UpdateCheckStatus::CheckFailed {
                "error"
            } else {
                "info"
            },
            response.message.clone(),
        );
        guard.updater = Some(response.clone());
        emit_state_changed(app, &guard);
    }

    response
}

async fn maybe_check_updates(app: &AppHandle, state: &SharedState) {
    let (enabled, interval_hours, last_check) = {
        let guard = state.inner.lock().unwrap();
        (
            guard.settings.update_checks_enabled,
            guard.settings.update_check_interval_hours,
            guard.last_update_check_ms,
        )
    };

    if !enabled {
        return;
    }

    let now = now_ms();
    if !update_check_due(last_check, now, interval_hours) {
        return;
    }

    let _ = perform_update_check(app, state, UpdateCheckTrigger::Scheduled).await;
}

async fn apply_pending_update(app: &AppHandle, state: &SharedState) -> Result<String, String> {
    if !updater_supported_platform() {
        return Err("In-app updater is enabled on macOS only for v1.".to_string());
    }

    let (update, settings, installation_id) = {
        let mut guard = state.inner.lock().unwrap();
        let Some(update) = guard.pending_update.take() else {
            return Err(
                "No pending update is ready to install. Run Check Updates first.".to_string(),
            );
        };
        push_log(
            &mut guard,
            "info",
            format!("Installing update {}...", update.version),
        );
        emit_state_changed(app, &guard);
        (
            update,
            guard.settings.clone(),
            guard.installation_id.clone(),
        )
    };

    if let Err(err) = update.download_and_install(|_, _| {}, || {}).await {
        let mut guard = state.inner.lock().unwrap();
        let message = format!("Update install failed: {err}");
        guard.updater = Some(update_response(
            now_ms(),
            UpdateCheckStatus::CheckFailed,
            None,
            Some(env!("CARGO_PKG_VERSION").to_string()),
            None,
            false,
            message.clone(),
        ));
        push_log(&mut guard, "error", message.clone());
        emit_state_changed(app, &guard);
        if settings.error_telemetry_enabled {
            capture_telemetry_error(
                &settings,
                &installation_id,
                &message,
                "updater",
                "install_failed",
            );
        }
        return Err(message);
    }

    {
        let mut guard = state.inner.lock().unwrap();
        guard.updater = Some(update_response(
            now_ms(),
            UpdateCheckStatus::UpToDate,
            None,
            Some(env!("CARGO_PKG_VERSION").to_string()),
            None,
            false,
            "Update installed successfully. Restarting...",
        ));
        guard.background_notified_update_version = None;
        push_log(
            &mut guard,
            "info",
            "Update installed successfully. Restarting...",
        );
        emit_state_changed(app, &guard);
    }

    app.restart()
}

fn maybe_emit_usage_heartbeat(app: &AppHandle, guard: &mut RuntimeState) {
    let now = now_ms();
    if !heartbeat_due(guard.last_usage_heartbeat_at_ms, now) {
        return;
    }

    capture_usage_event(
        &guard.settings,
        &guard.installation_id,
        "heartbeat",
        "poll_engine",
        "Usage heartbeat emitted",
    );
    guard.last_usage_heartbeat_at_ms = Some(now);
    push_log(&mut *guard, "info", "Usage heartbeat emitted");
    let _ = save_persistent(app, guard);
}

async fn perform_poll(app: AppHandle, state: SharedState, reason: &str) {
    let settings = {
        let mut guard = state.inner.lock().unwrap();
        if guard.in_flight {
            return;
        }
        guard.in_flight = true;
        let settings = guard.settings.clone();
        push_log(
            &mut guard,
            "info",
            format!(
                "Polling status ({reason}) from {}",
                settings.status_endpoint_url
            ),
        );
        emit_state_changed(&app, &guard);
        settings
    };

    let outcome = fetch_status(&settings).await;

    let mut notify: Option<(String, String)> = None;
    {
        let mut guard = state.inner.lock().unwrap();
        guard.in_flight = false;

        match outcome {
            Ok(status) => {
                if matches!(status.transport, FetchTransport::CurlFallback) {
                    push_log(
                        &mut guard,
                        "warn",
                        "Polling succeeded via curl fallback (reqwest connect failed)",
                    );
                } else {
                    push_log(&mut guard, "info", "Polling succeeded via reqwest");
                }

                let previous = guard.snapshot.last_known_state.clone();
                let previous_failures = guard.snapshot.consecutive_failures;
                let current = status.effective_state.clone();

                guard.snapshot.last_success_at = Some(status.fetched_at_ms);
                guard.snapshot.consecutive_failures = 0;
                guard.snapshot.last_error_summary = None;
                guard.snapshot.last_known_state = Some(current.clone());

                if previous_failures > 0 {
                    if let Some(event_id) = capture_telemetry_message(
                        &guard.settings,
                        &guard.installation_id,
                        "failure_streak_recovered",
                        "poll_engine",
                        sentry::Level::Info,
                        &format!(
                            "Polling recovered after {previous_failures} consecutive failures"
                        ),
                    ) {
                        push_log(
                            &mut guard,
                            "info",
                            format!("Telemetry recovery event sent: {}", event_id),
                        );
                    }
                }

                if let Some(prev) = previous {
                    if prev != current {
                        guard.transitions.push_back(TransitionEntry {
                            from: prev.clone(),
                            to: current.clone(),
                            detected_at: status.fetched_at_ms,
                            source_updated_at: status.source_timestamp_ms,
                        });

                        while guard.transitions.len() > TRANSITIONS_LIMIT {
                            guard.transitions.pop_front();
                        }

                        let should_notify = match guard.settings.notification_policy {
                            NotificationPolicy::Flip => true,
                            NotificationPolicy::NoToYes => current == "yes",
                        };

                        if should_notify {
                            notify = Some((
                                "ResetPing: status changed".to_string(),
                                format!("Codex reset status changed from {prev} to {current}."),
                            ));
                        }

                        capture_usage_event(
                            &guard.settings,
                            &guard.installation_id,
                            "transition_detected",
                            "poll_engine",
                            &format!("State transition detected: {prev} -> {current}"),
                        );

                        push_log(
                            &mut guard,
                            "info",
                            format!(
                                "State transition detected: {prev} -> {current} (configured={})",
                                status.configured
                            ),
                        );
                    }
                } else {
                    push_log(
                        &mut guard,
                        "info",
                        format!("Initial state detected: {}", current),
                    );

                    if guard.settings.notify_initial_state {
                        notify = Some((
                            "ResetPing: initial status".to_string(),
                            format!("Current Codex reset status is {current}."),
                        ));
                    }
                }
            }
            Err(err) => {
                guard.snapshot.consecutive_failures =
                    guard.snapshot.consecutive_failures.saturating_add(1);
                guard.snapshot.last_error_summary = Some(err.clone());
                if should_emit_poll_failure_telemetry(guard.snapshot.consecutive_failures) {
                    if let Some(sentry_event_id) = capture_telemetry_message(
                        &guard.settings,
                        &guard.installation_id,
                        "error",
                        "poll_engine",
                        sentry::Level::Error,
                        &format!(
                            "Polling failed (consecutive={}): {err}",
                            guard.snapshot.consecutive_failures
                        ),
                    ) {
                        push_log(
                            &mut guard,
                            "info",
                            format!("Telemetry error event sent: {}", sentry_event_id),
                        );
                    }
                }

                push_log(&mut guard, "error", format!("Polling failed: {err}"));

                if guard.snapshot.consecutive_failures == FAILURE_THRESHOLD {
                    push_log(
                        &mut guard,
                        "warn",
                        format!(
                            "Endpoint health degraded after {} consecutive failures",
                            FAILURE_THRESHOLD
                        ),
                    );
                }
            }
        }

        update_tray_tooltip(&app, &guard.snapshot);
        emit_state_changed(&app, &guard);
    }

    let mut guard = state.inner.lock().unwrap();
    if let Err(err) = save_persistent(&app, &guard) {
        push_log(
            &mut guard,
            "error",
            format!("Failed to persist state: {err}"),
        );
        if guard.settings.error_telemetry_enabled {
            capture_telemetry_error(
                &guard.settings,
                &guard.installation_id,
                &format!("State persistence failed after poll: {err}"),
                "state_store",
                "persist_failed",
            );
        }
    }
    drop(guard);

    if let Some((title, body)) = notify {
        send_notification(&app, &title, &body);
    }
}

fn spawn_poll_loop(app: AppHandle, state: SharedState) {
    tauri::async_runtime::spawn(async move {
        perform_poll(app.clone(), state.clone(), "startup").await;

        loop {
            {
                let mut guard = state.inner.lock().unwrap();
                maybe_emit_usage_heartbeat(&app, &mut guard);
                emit_state_changed(&app, &guard);
            }
            maybe_check_updates(&app, &state).await;

            let base_interval = {
                let guard = state.inner.lock().unwrap();
                if low_power_mode_active() {
                    guard.settings.low_power_poll_interval_sec
                } else {
                    guard.settings.poll_interval_sec
                }
            };

            let jitter = rand::rng().random_range(0_u64..=5_u64);
            sleep(Duration::from_secs(base_interval.saturating_add(jitter))).await;

            perform_poll(app.clone(), state.clone(), "scheduled").await;
        }
    });
}

fn build_tray(app: &AppHandle) -> Result<(), String> {
    let open_settings =
        MenuItem::with_id(app, "open-settings", "Open Settings", true, None::<&str>)
            .map_err(|e| format!("create menu item failed: {e}"))?;
    let force_refresh =
        MenuItem::with_id(app, "force-refresh", "Force Refresh", true, None::<&str>)
            .map_err(|e| format!("create menu item failed: {e}"))?;
    let test_notification = MenuItem::with_id(
        app,
        "test-notification",
        "Send Test Notification",
        true,
        None::<&str>,
    )
    .map_err(|e| format!("create menu item failed: {e}"))?;
    let quit = MenuItem::with_id(app, "quit", "Quit ResetPing", true, None::<&str>)
        .map_err(|e| format!("create menu item failed: {e}"))?;

    let separator =
        PredefinedMenuItem::separator(app).map_err(|e| format!("separator failed: {e}"))?;

    let menu = Menu::with_items(
        app,
        &[
            &open_settings,
            &force_refresh,
            &test_notification,
            &separator,
            &quit,
        ],
    )
    .map_err(|e| format!("menu creation failed: {e}"))?;

    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("ResetPing: starting…")
        .menu(&menu);

    #[cfg(target_os = "macos")]
    {
        tray = tray.title("RP");
    }

    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }

    tray.show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open-settings" => show_settings_window(app),
            "force-refresh" => {
                let app_handle = app.clone();
                let shared = app_handle.state::<SharedState>().inner.clone();
                tauri::async_runtime::spawn(async move {
                    perform_poll(app_handle.clone(), SharedState { inner: shared }, "manual").await;
                });
            }
            "test-notification" => {
                let shared = app.state::<SharedState>();
                let guard = shared.inner.lock().unwrap();
                send_notification(
                    app,
                    "ResetPing: notification test",
                    &format!(
                        "Notifications are working. {}",
                        build_status_line(&guard.snapshot)
                    ),
                );
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)
        .map_err(|e| format!("tray build failed: {e}"))?;

    Ok(())
}

#[tauri::command]
fn get_app_state(state: State<'_, SharedState>) -> Result<AppStateResponse, String> {
    let guard = state.inner.lock().unwrap();
    Ok(AppStateResponse {
        settings: guard.settings.clone(),
        snapshot: guard.snapshot.clone(),
        transitions: guard.transitions.iter().cloned().collect(),
        logs: guard.logs.iter().cloned().collect(),
        health: health_label(&guard.snapshot),
        installation_id: guard.installation_id.clone(),
        frontend_telemetry_dsn: guard.sentry_dsn.clone(),
        updater: guard.updater.clone(),
    })
}

#[tauri::command]
fn update_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let settings = sanitize_settings(settings);
    let any_telemetry_enabled = telemetry_enabled(&settings);
    let previous_settings = {
        let guard = state.inner.lock().unwrap();
        guard.settings.clone()
    };

    if settings.launch_at_login != previous_settings.launch_at_login {
        if let Err(err) = set_autostart(&app, settings.launch_at_login) {
            if settings.error_telemetry_enabled {
                let installation_id = {
                    let guard = state.inner.lock().unwrap();
                    guard.installation_id.clone()
                };
                capture_telemetry_error(
                    &settings,
                    &installation_id,
                    &format!("Autostart update failed: {err}"),
                    "autostart",
                    "update_failed",
                );
            }
            return Err(err);
        }
    }

    {
        let mut guard = state.inner.lock().unwrap();
        guard.settings = settings.clone();
        push_log(&mut guard, "info", "Settings updated");
        emit_state_changed(&app, &guard);
        if let Err(err) = save_persistent(&app, &guard) {
            guard.settings = previous_settings.clone();
            push_log(
                &mut guard,
                "warn",
                "Settings change rolled back after persistence failure",
            );
            emit_state_changed(&app, &guard);
            let _ = save_persistent(&app, &guard);
            if settings.launch_at_login != previous_settings.launch_at_login {
                let _ = set_autostart(&app, previous_settings.launch_at_login);
            }
            if guard.settings.error_telemetry_enabled {
                capture_telemetry_error(
                    &guard.settings,
                    &guard.installation_id,
                    &format!("Failed to save settings: {err}"),
                    "state_store",
                    "save_settings_failed",
                );
            }
            return Err(err);
        }
    }
    let dsn = {
        let guard = state.inner.lock().unwrap();
        guard.sentry_dsn.clone()
    };
    init_telemetry(any_telemetry_enabled, dsn.as_deref());

    Ok(settings)
}

#[tauri::command]
async fn manual_refresh(app: AppHandle, state: State<'_, SharedState>) -> Result<(), String> {
    perform_poll(
        app,
        SharedState {
            inner: state.inner.clone(),
        },
        "manual-command",
    )
    .await;
    Ok(())
}

#[tauri::command]
fn send_test_notification_cmd(app: AppHandle, state: State<'_, SharedState>) {
    let guard = state.inner.lock().unwrap();
    send_notification(
        &app,
        "ResetPing: notification test",
        &format!(
            "Notifications are working. {}",
            build_status_line(&guard.snapshot)
        ),
    );
}

#[tauri::command]
fn get_recent_logs(state: State<'_, SharedState>) -> Vec<LogEntry> {
    let guard = state.inner.lock().unwrap();
    guard.logs.iter().cloned().collect()
}

#[tauri::command]
async fn check_for_updates(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> Result<UpdateCheckResponse, String> {
    Ok(perform_update_check(
        &app,
        &SharedState {
            inner: state.inner.clone(),
        },
        UpdateCheckTrigger::Manual,
    )
    .await)
}

#[tauri::command]
async fn install_update(app: AppHandle, state: State<'_, SharedState>) -> Result<String, String> {
    apply_pending_update(
        &app,
        &SharedState {
            inner: state.inner.clone(),
        },
    )
    .await
}

#[tauri::command]
fn send_test_telemetry_event(state: State<'_, SharedState>) -> Result<String, String> {
    let guard = state.inner.lock().unwrap();
    if !guard.settings.error_telemetry_enabled {
        return Err("Error telemetry is disabled in settings.".to_string());
    }
    let installation_id = guard.installation_id.clone();
    drop(guard);

    if SENTRY_GUARD.get().is_none() {
        return Err("Sentry is not initialized. Set SENTRY_DSN and restart app.".to_string());
    }

    let id = sentry::with_scope(
        |scope| {
            scope.set_tag("event_type", "error");
            scope.set_tag("component", "ui");
            scope.set_tag("error_kind", "manual_telemetry_test");
            scope.set_tag("platform", std::env::consts::OS.to_string());
            scope.set_tag("app_version", env!("CARGO_PKG_VERSION").to_string());
            scope.set_tag("build_channel", build_channel().to_string());
            scope.set_tag("installation_id", installation_id.clone());
            scope.set_extra("source", "settings_button".into());
        },
        || {
            let err = std::io::Error::other("Everything is on fire! (ResetPing telemetry test)");
            sentry::capture_error(&err)
        },
    );
    Ok(id.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let persistent = load_persistent(&app_handle);
            let settings = sanitize_settings(persistent.settings);
            let installation_id = persistent
                .installation_id
                .unwrap_or_else(default_installation_id);
            let sentry_dsn = resolve_sentry_dsn();
            let telemetry_active = telemetry_enabled(&settings);

            let mut state = RuntimeState {
                settings,
                snapshot: persistent.snapshot,
                transitions: VecDeque::from(persistent.transitions),
                logs: VecDeque::new(),
                in_flight: false,
                last_update_check_ms: None,
                pending_update: None,
                updater: None,
                background_notified_update_version: None,
                installation_id,
                last_usage_heartbeat_at_ms: persistent.last_usage_heartbeat_at_ms,
                sentry_dsn,
            };

            push_log(&mut state, "info", "ResetPing started");
            push_log(
                &mut state,
                "info",
                format!("HTTP backend selected: {}", http_backend_label()),
            );

            let shared = SharedState {
                inner: Arc::new(Mutex::new(state)),
            };

            app.manage(shared.clone());

            {
                let guard = shared.inner.lock().unwrap();
                init_telemetry(telemetry_active, guard.sentry_dsn.as_deref());
                capture_usage_event(
                    &guard.settings,
                    &guard.installation_id,
                    "app_open",
                    "app_setup",
                    "ResetPing app open event",
                );
            }

            #[cfg(target_os = "macos")]
            if let Err(e) = app_handle.set_activation_policy(ActivationPolicy::Accessory) {
                let message = format!("failed to set activation policy: {e}");
                let guard = shared.inner.lock().unwrap();
                capture_telemetry_error(
                    &guard.settings,
                    &guard.installation_id,
                    &message,
                    "app_setup",
                    "activation_policy_failed",
                );
                return Err(message.into());
            }

            if let Err(err) = set_autostart(&app_handle, {
                let guard = shared.inner.lock().unwrap();
                guard.settings.launch_at_login
            }) {
                let guard = shared.inner.lock().unwrap();
                capture_telemetry_error(
                    &guard.settings,
                    &guard.installation_id,
                    &format!("Autostart setup failed at startup: {err}"),
                    "autostart",
                    "startup_setup_failed",
                );
                return Err(err.into());
            }

            if let Err(err) = build_tray(&app_handle) {
                let guard = shared.inner.lock().unwrap();
                capture_telemetry_error(
                    &guard.settings,
                    &guard.installation_id,
                    &format!("Tray setup failed: {err}"),
                    "tray",
                    "startup_setup_failed",
                );
                return Err(err.into());
            }

            if let Some(win) = app_handle.get_webview_window("main") {
                let _ = win.hide();
            }

            {
                let mut guard = shared.inner.lock().unwrap();
                maybe_emit_usage_heartbeat(&app_handle, &mut guard);
                update_tray_tooltip(&app_handle, &guard.snapshot);
                emit_state_changed(&app_handle, &guard);
            }

            spawn_poll_loop(app_handle, shared);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_state,
            update_settings,
            manual_refresh,
            send_test_notification_cmd,
            get_recent_logs,
            check_for_updates,
            install_update,
            send_test_telemetry_event,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_non_no_to_yes() {
        let payload = StatusApiPayload {
            state: "maybe".to_string(),
            configured: true,
            updated_at: Some(123),
            error: None,
        };
        let effective = if payload.state == "no" { "no" } else { "yes" };
        assert_eq!(effective, "yes");
    }

    #[test]
    fn clamps_settings_bounds() {
        let settings = AppSettings {
            poll_interval_sec: 1,
            low_power_poll_interval_sec: 5_000,
            http_timeout_ms: 100,
            notification_policy: NotificationPolicy::Flip,
            notify_initial_state: true,
            launch_at_login: false,
            update_checks_enabled: false,
            update_check_interval_hours: 1_000,
            status_endpoint_url: "".to_string(),
            error_telemetry_enabled: true,
            usage_telemetry_enabled: false,
            legacy_telemetry_enabled: None,
        };

        let sanitized = sanitize_settings(settings);
        assert_eq!(sanitized.poll_interval_sec, 30);
        assert_eq!(sanitized.low_power_poll_interval_sec, 900);
        assert_eq!(sanitized.http_timeout_ms, 1_000);
        assert_eq!(sanitized.update_check_interval_hours, 24 * 30);
        assert_eq!(sanitized.status_endpoint_url, DEFAULT_ENDPOINT);
    }

    #[test]
    fn detects_degraded_health() {
        let mut snapshot = RuntimeSnapshot::default();
        snapshot.consecutive_failures = FAILURE_THRESHOLD;
        assert!(is_degraded(&snapshot));
    }

    #[test]
    fn migrates_legacy_telemetry_toggle() {
        let settings = AppSettings {
            poll_interval_sec: default_poll_interval_sec(),
            low_power_poll_interval_sec: default_low_power_poll_interval_sec(),
            http_timeout_ms: default_http_timeout_ms(),
            notification_policy: NotificationPolicy::Flip,
            notify_initial_state: true,
            launch_at_login: false,
            update_checks_enabled: false,
            update_check_interval_hours: default_update_check_interval_hours(),
            status_endpoint_url: default_endpoint_url(),
            error_telemetry_enabled: true,
            usage_telemetry_enabled: false,
            legacy_telemetry_enabled: Some(false),
        };

        let sanitized = sanitize_settings(settings);
        assert!(!sanitized.error_telemetry_enabled);
        assert!(!sanitized.usage_telemetry_enabled);
        assert!(sanitized.legacy_telemetry_enabled.is_none());
    }

    #[test]
    fn heartbeat_due_logic_respects_interval() {
        assert!(heartbeat_due(None, HEARTBEAT_INTERVAL_MS));
        assert!(!heartbeat_due(
            Some(1_000),
            1_000 + HEARTBEAT_INTERVAL_MS - 1
        ));
        assert!(heartbeat_due(Some(1_000), 1_000 + HEARTBEAT_INTERVAL_MS));
    }

    #[test]
    fn poll_failure_telemetry_thresholds_are_throttled() {
        assert!(should_emit_poll_failure_telemetry(1));
        assert!(!should_emit_poll_failure_telemetry(2));
        assert!(should_emit_poll_failure_telemetry(5));
        assert!(!should_emit_poll_failure_telemetry(6));
        assert!(should_emit_poll_failure_telemetry(20));
        assert!(should_emit_poll_failure_telemetry(100));
        assert!(!should_emit_poll_failure_telemetry(101));
    }

    #[test]
    fn update_check_due_respects_interval() {
        let now = 20_000_000;
        assert!(update_check_due(None, now, 24));
        assert!(!update_check_due(Some(now - (2 * 60 * 60 * 1000)), now, 3));
        assert!(update_check_due(Some(now - (4 * 60 * 60 * 1000)), now, 3));
    }

    #[test]
    fn unsupported_platform_response_shape_is_stable() {
        let response = update_response(
            123,
            UpdateCheckStatus::UnsupportedPlatform,
            None,
            Some("0.1.0".to_string()),
            None,
            false,
            "In-app updater is enabled on macOS only for v1.",
        );

        assert_eq!(response.checked_at, 123);
        assert_eq!(response.status, UpdateCheckStatus::UnsupportedPlatform);
        assert!(!response.install_ready);
        assert_eq!(response.current_version.as_deref(), Some("0.1.0"));
    }
}
