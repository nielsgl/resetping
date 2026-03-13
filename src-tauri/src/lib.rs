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
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    ActivationPolicy, AppHandle, Emitter, Manager, State,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt;
use tokio::time::sleep;

const DEFAULT_ENDPOINT: &str = "https://hascodexratelimitreset.today/api/status";
const APP_USER_AGENT: &str = "ResetPing/0.1.0 (+https://github.com/niels-vg/codex-reset-notifier)";
const STATE_FILE: &str = "state.json";
const TRANSITIONS_LIMIT: usize = 100;
const LOG_LIMIT: usize = 200;
const FAILURE_THRESHOLD: u32 = 5;
static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();

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
    telemetry_enabled: bool,
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
            telemetry_enabled: true,
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
}

#[derive(Debug, Clone, Serialize)]
struct UpdateCheckResponse {
    checked_at: u64,
    result: String,
}

#[derive(Debug, Clone, Deserialize)]
struct StatusApiPayload {
    state: String,
    configured: bool,
    #[serde(rename = "updatedAt")]
    updated_at: Option<u64>,
}

#[derive(Debug, Clone)]
struct NormalizedStatus {
    effective_state: String,
    source_timestamp_ms: Option<u64>,
    fetched_at_ms: u64,
    configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct PersistentData {
    settings: AppSettings,
    snapshot: RuntimeSnapshot,
    transitions: Vec<TransitionEntry>,
}

struct RuntimeState {
    settings: AppSettings,
    snapshot: RuntimeSnapshot,
    transitions: VecDeque<TransitionEntry>,
    logs: VecDeque<LogEntry>,
    in_flight: bool,
    last_update_check_ms: Option<u64>,
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

fn sanitize_settings(mut settings: AppSettings) -> AppSettings {
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

fn init_telemetry(enabled: bool) {
    if !enabled {
        return;
    }

    let _ = dotenvy::dotenv();

    let Ok(dsn) = std::env::var("SENTRY_DSN") else {
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

fn endpoint_host(input: &str) -> Option<String> {
    reqwest::Url::parse(input)
        .ok()
        .and_then(|u| u.host_str().map(ToString::to_string))
}

fn capture_telemetry_error(message: &str, component: &str, error_kind: &str) {
    if SENTRY_GUARD.get().is_some() {
        sentry::with_scope(
            |scope| {
                scope.set_tag("event_type", "backend_error");
                scope.set_tag("component", component.to_string());
                scope.set_tag("error_kind", error_kind.to_string());
            },
            || {
                let err = std::io::Error::other(message.to_string());
                sentry::capture_error(&err)
            },
        );
    }
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
    let client = Client::builder()
        .timeout(Duration::from_millis(settings.http_timeout_ms))
        .build()
        .map_err(|e| format!("failed to build client: {e}"))?;

    let response = client
        .get(settings.status_endpoint_url.clone())
        .header("cache-control", "no-store")
        .header("user-agent", APP_USER_AGENT)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(format!("request failed with status {status}"));
    }

    let payload: StatusApiPayload = response
        .json()
        .await
        .map_err(|e| format!("invalid response body: {e}"))?;

    let effective_state = if payload.state == "no" { "no" } else { "yes" };

    Ok(NormalizedStatus {
        effective_state: effective_state.to_string(),
        source_timestamp_ms: payload.updated_at,
        fetched_at_ms: now_ms(),
        configured: payload.configured,
    })
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
    let due_ms = interval_hours.saturating_mul(60 * 60 * 1000);
    if last_check
        .map(|last| now.saturating_sub(last) < due_ms)
        .unwrap_or(false)
    {
        return;
    }

    {
        let mut guard = state.inner.lock().unwrap();
        guard.last_update_check_ms = Some(now);
        push_log(
            &mut guard,
            "info",
            "Background update check requested (manual install flow in v1)",
        );
        emit_state_changed(app, &guard);
    }
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
                let previous = guard.snapshot.last_known_state.clone();
                let current = status.effective_state.clone();

                guard.snapshot.last_success_at = Some(status.fetched_at_ms);
                guard.snapshot.consecutive_failures = 0;
                guard.snapshot.last_error_summary = None;
                guard.snapshot.last_known_state = Some(current.clone());

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
                if guard.settings.telemetry_enabled {
                    sentry::with_scope(
                        |scope| {
                            scope.set_tag("event_type", "backend_error");
                            scope.set_tag("component", "poll_engine");
                            scope.set_tag("error_kind", "poll_failed");
                            if let Some(host) = endpoint_host(&guard.settings.status_endpoint_url) {
                                scope.set_extra("endpoint_host", host.into());
                            }
                        },
                        || {
                            let error = std::io::Error::other(format!("Polling failed: {err}"));
                            sentry::capture_error(&error)
                        },
                    );
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
        if guard.settings.telemetry_enabled {
            capture_telemetry_error(
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
    })
}

#[tauri::command]
fn update_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let settings = sanitize_settings(settings);
    let telemetry_enabled = settings.telemetry_enabled;

    {
        let mut guard = state.inner.lock().unwrap();
        guard.settings = settings.clone();
        push_log(&mut guard, "info", "Settings updated");
        emit_state_changed(&app, &guard);
        if let Err(err) = save_persistent(&app, &guard) {
            if telemetry_enabled {
                capture_telemetry_error(
                    &format!("Failed to save settings: {err}"),
                    "state_store",
                    "save_settings_failed",
                );
            }
            return Err(err);
        }
    }

    if let Err(err) = set_autostart(&app, settings.launch_at_login) {
        if telemetry_enabled {
            capture_telemetry_error(
                &format!("Autostart update failed: {err}"),
                "autostart",
                "update_failed",
            );
        }
        return Err(err);
    }
    init_telemetry(settings.telemetry_enabled);

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
fn check_for_updates(state: State<'_, SharedState>) -> UpdateCheckResponse {
    let mut guard = state.inner.lock().unwrap();
    let now = now_ms();
    guard.last_update_check_ms = Some(now);
    push_log(
        &mut guard,
        "info",
        "Manual update check requested (GitHub release check to be expanded)",
    );

    UpdateCheckResponse {
        checked_at: now,
        result: "No update detected in v1 stub check".to_string(),
    }
}

#[tauri::command]
fn send_test_telemetry_event(state: State<'_, SharedState>) -> Result<String, String> {
    let guard = state.inner.lock().unwrap();
    if !guard.settings.telemetry_enabled {
        return Err("Telemetry is disabled in settings.".to_string());
    }
    drop(guard);

    if SENTRY_GUARD.get().is_none() {
        return Err("Sentry is not initialized. Set SENTRY_DSN and restart app.".to_string());
    }

    let id = sentry::with_scope(
        |scope| {
            scope.set_tag("event_type", "manual_telemetry_test");
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
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let persistent = load_persistent(&app_handle);

            let mut state = RuntimeState {
                settings: sanitize_settings(persistent.settings),
                snapshot: persistent.snapshot,
                transitions: VecDeque::from(persistent.transitions),
                logs: VecDeque::new(),
                in_flight: false,
                last_update_check_ms: None,
            };

            push_log(&mut state, "info", "ResetPing started");

            let shared = SharedState {
                inner: Arc::new(Mutex::new(state)),
            };

            app.manage(shared.clone());

            init_telemetry({
                let guard = shared.inner.lock().unwrap();
                guard.settings.telemetry_enabled
            });

            #[cfg(target_os = "macos")]
            if let Err(e) = app_handle.set_activation_policy(ActivationPolicy::Accessory) {
                let message = format!("failed to set activation policy: {e}");
                capture_telemetry_error(&message, "app_setup", "activation_policy_failed");
                return Err(message.into());
            }

            if let Err(err) = set_autostart(&app_handle, {
                let guard = shared.inner.lock().unwrap();
                guard.settings.launch_at_login
            }) {
                capture_telemetry_error(
                    &format!("Autostart setup failed at startup: {err}"),
                    "autostart",
                    "startup_setup_failed",
                );
                return Err(err.into());
            }

            if let Err(err) = build_tray(&app_handle) {
                capture_telemetry_error(
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
                let guard = shared.inner.lock().unwrap();
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
            telemetry_enabled: true,
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
}
