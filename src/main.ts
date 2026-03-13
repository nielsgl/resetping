import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import "./styles.css";
import { captureUiError, initFrontendTelemetry } from "./utils/telemetry";
import { buildLogExport, formatTimestamp } from "./utils/time";
import { shouldShowInstallButton, type UpdateCheckResponse } from "./utils/updater";

const DEFAULT_ENDPOINT_URL = "https://hascodexratelimitreset.today/api/status";
const IS_DEV_BUILD = import.meta.env.DEV;

type NotificationPolicy = "flip" | "no_to_yes";

type AppSettings = {
  poll_interval_sec: number;
  low_power_poll_interval_sec: number;
  http_timeout_ms: number;
  notification_policy: NotificationPolicy;
  notify_initial_state: boolean;
  launch_at_login: boolean;
  update_checks_enabled: boolean;
  update_check_interval_hours: number;
  status_endpoint_url: string;
  error_telemetry_enabled: boolean;
  usage_telemetry_enabled: boolean;
};

type RuntimeSnapshot = {
  last_known_state: string | null;
  last_success_at: number | null;
  consecutive_failures: number;
  last_error_summary: string | null;
};

type TransitionEntry = {
  from: string;
  to: string;
  detected_at: number;
  source_updated_at: number | null;
};

type LogEntry = {
  timestamp_ms: number;
  level: string;
  message: string;
};

type AppStateResponse = {
  settings: AppSettings;
  snapshot: RuntimeSnapshot;
  transitions: TransitionEntry[];
  logs: LogEntry[];
  health: string;
  installation_id: string;
  frontend_telemetry_dsn: string | null;
  updater: UpdateCheckResponse | null;
};

let currentState: AppStateResponse | null = null;
let messageTimerSettings: number | null = null;
let messageTimerDiagnostics: number | null = null;
let previousTransitionCount = 0;
let isSettingsFormDirty = false;
let isProgrammaticFormUpdate = false;

const app = document.querySelector<HTMLDivElement>("#app");
if (!app) {
  throw new Error("Missing app root");
}

app.innerHTML = `
  <main class="shell">
    <header class="hero">
      <h1>ResetPing</h1>
      <p>Codex reset notifier</p>
      <div class="hero-actions">
        <button id="refresh-now">Force Refresh</button>
        <button id="test-notification">Send Test Notification</button>
      </div>
    </header>

    <section class="panel status-panel">
      <h2>Status</h2>
      <div id="status-cards" class="cards"></div>
    </section>

    <section class="panel">
      <h2>Settings</h2>
      <form id="settings-form" class="settings-grid">
        <label>Polling interval (sec)
          <input type="number" id="poll_interval_sec" min="30" max="900" required />
        </label>

        <label>Low power interval (sec)
          <input type="number" id="low_power_poll_interval_sec" min="60" max="900" required />
        </label>

        <label>HTTP timeout (ms)
          <input type="number" id="http_timeout_ms" min="1000" max="30000" required />
        </label>

        <label>Notification policy
          <select id="notification_policy">
            <option value="flip">Any state flip</option>
            <option value="no_to_yes">Only no -> yes</option>
          </select>
        </label>

        <label>Update check interval (hours)
          <input type="number" id="update_check_interval_hours" min="1" max="720" required />
        </label>

        <label>Status endpoint URL
          <input type="url" id="status_endpoint_url" required />
        </label>

        <label class="checkbox-row"><input type="checkbox" id="notify_initial_state" /> Notify initial state</label>
        <label class="checkbox-row"><input type="checkbox" id="launch_at_login" /> Launch at login</label>
        <label class="checkbox-row"><input type="checkbox" id="update_checks_enabled" /> Enable background update checks</label>
        <label class="checkbox-row"><input type="checkbox" id="error_telemetry_enabled" /> Enable anonymous crash/error telemetry</label>
        <label class="checkbox-row"><input type="checkbox" id="usage_telemetry_enabled" /> Enable anonymous usage analytics</label>

        <div class="form-actions">
          <button type="submit">Save Settings</button>
          <button id="check-updates" type="button">Check Updates</button>
          <button id="install-update" type="button" hidden>Install Update</button>
          ${IS_DEV_BUILD ? '<button id="test-telemetry" type="button">Send Telemetry Test</button>' : ""}
          ${IS_DEV_BUILD ? '<button id="test-ui-error" type="button">Send UI Error Test</button>' : ""}
          <button id="reset-endpoint" type="button">Reset Endpoint URL</button>
        </div>
      </form>
      <p id="save-message" class="status-message"></p>
      <p id="update-status" class="status-message status-inline"></p>
    </section>

    <section class="panel split">
      <div>
        <h2>Recent Transitions</h2>
        <ul id="transition-list" class="list"></ul>
      </div>
      <div>
        <h2>Diagnostics</h2>
        <button id="copy-logs" type="button">Copy Logs</button>
        <p id="diagnostics-message" class="status-message status-inline"></p>
        <ul id="log-list" class="list logs"></ul>
      </div>
    </section>
  </main>
`;

function setStatusCards(snapshot: RuntimeSnapshot, health: string): void {
  const host = document.querySelector<HTMLDivElement>("#status-cards");
  if (!host) return;
  host.replaceChildren();

  const addCard = (label: string, value: string): void => {
    const article = document.createElement("article");
    article.className = "card";
    const h3 = document.createElement("h3");
    h3.textContent = label;
    const strong = document.createElement("strong");
    strong.textContent = value;
    article.append(h3, strong);
    host.append(article);
  };

  addCard("Current State", snapshot.last_known_state ?? "unknown");
  addCard("Health", health);
  addCard("Last Success", formatTimestamp(snapshot.last_success_at));
  addCard("Consecutive Failures", String(snapshot.consecutive_failures));
}

function renderTransitions(transitions: TransitionEntry[]): void {
  const list = document.querySelector<HTMLUListElement>("#transition-list");
  if (!list) return;
  list.replaceChildren();
  if (transitions.length === 0) {
    const empty = document.createElement("li");
    empty.className = "muted";
    empty.textContent = "No transitions yet.";
    list.append(empty);
    return;
  }

  for (const transition of transitions.slice().reverse()) {
    const item = document.createElement("li");
    const strong = document.createElement("strong");
    strong.textContent = `${transition.from} -> ${transition.to}`;
    const br = document.createElement("br");
    const span = document.createElement("span");
    span.textContent = formatTimestamp(transition.detected_at);
    item.append(strong, br, span);
    list.append(item);
  }
}

function renderLogs(logs: LogEntry[]): void {
  const list = document.querySelector<HTMLUListElement>("#log-list");
  if (!list) return;
  list.replaceChildren();
  if (logs.length === 0) {
    const empty = document.createElement("li");
    empty.className = "muted";
    empty.textContent = "No logs yet.";
    list.append(empty);
    return;
  }

  for (const log of logs.slice().reverse()) {
    const item = document.createElement("li");
    const strong = document.createElement("strong");
    strong.textContent = `[${log.level.toUpperCase()}]`;
    item.append(strong, document.createTextNode(` ${log.message}`), document.createElement("br"));
    const span = document.createElement("span");
    span.textContent = formatTimestamp(log.timestamp_ms);
    item.append(span);
    list.append(item);
  }
}

function setFormValues(settings: AppSettings): void {
  isProgrammaticFormUpdate = true;
  (document.querySelector("#poll_interval_sec") as HTMLInputElement).value = String(settings.poll_interval_sec);
  (document.querySelector("#low_power_poll_interval_sec") as HTMLInputElement).value = String(settings.low_power_poll_interval_sec);
  (document.querySelector("#http_timeout_ms") as HTMLInputElement).value = String(settings.http_timeout_ms);
  (document.querySelector("#notification_policy") as HTMLSelectElement).value = settings.notification_policy;
  (document.querySelector("#notify_initial_state") as HTMLInputElement).checked = settings.notify_initial_state;
  (document.querySelector("#launch_at_login") as HTMLInputElement).checked = settings.launch_at_login;
  (document.querySelector("#update_checks_enabled") as HTMLInputElement).checked = settings.update_checks_enabled;
  (document.querySelector("#update_check_interval_hours") as HTMLInputElement).value = String(settings.update_check_interval_hours);
  (document.querySelector("#status_endpoint_url") as HTMLInputElement).value = settings.status_endpoint_url;
  (document.querySelector("#error_telemetry_enabled") as HTMLInputElement).checked = settings.error_telemetry_enabled;
  (document.querySelector("#usage_telemetry_enabled") as HTMLInputElement).checked = settings.usage_telemetry_enabled;
  isProgrammaticFormUpdate = false;
}

function readSettingsFromForm(): AppSettings {
  return {
    poll_interval_sec: Number((document.querySelector("#poll_interval_sec") as HTMLInputElement).value),
    low_power_poll_interval_sec: Number(
      (document.querySelector("#low_power_poll_interval_sec") as HTMLInputElement).value,
    ),
    http_timeout_ms: Number((document.querySelector("#http_timeout_ms") as HTMLInputElement).value),
    notification_policy: (document.querySelector("#notification_policy") as HTMLSelectElement)
      .value as NotificationPolicy,
    notify_initial_state: (document.querySelector("#notify_initial_state") as HTMLInputElement).checked,
    launch_at_login: (document.querySelector("#launch_at_login") as HTMLInputElement).checked,
    update_checks_enabled: (document.querySelector("#update_checks_enabled") as HTMLInputElement).checked,
    update_check_interval_hours: Number(
      (document.querySelector("#update_check_interval_hours") as HTMLInputElement).value,
    ),
    status_endpoint_url: (document.querySelector("#status_endpoint_url") as HTMLInputElement).value,
    error_telemetry_enabled: (document.querySelector("#error_telemetry_enabled") as HTMLInputElement).checked,
    usage_telemetry_enabled: (document.querySelector("#usage_telemetry_enabled") as HTMLInputElement).checked,
  };
}

function renderAll(state: AppStateResponse, options: { forceFormSync?: boolean } = {}): void {
  initFrontendTelemetry({
    dsn: state.frontend_telemetry_dsn ?? undefined,
    installationId: state.installation_id,
    errorTelemetryEnabled: state.settings.error_telemetry_enabled,
  });
  currentState = state;
  setStatusCards(state.snapshot, state.health);
  if (options.forceFormSync || !isSettingsFormDirty) {
    setFormValues(state.settings);
  }
  renderTransitions(state.transitions);
  renderLogs(state.logs);
  renderUpdateState(state.updater);

  if (state.transitions.length > previousTransitionCount && previousTransitionCount > 0) {
    const newest = state.transitions[state.transitions.length - 1];
    flashMessage(
      `Transition detected: ${newest.from} -> ${newest.to}. If this window is focused, macOS may hide banners.`,
      "info",
      "save-message",
    );
  }
  previousTransitionCount = state.transitions.length;
}

function renderUpdateState(update: UpdateCheckResponse | null): void {
  const status = document.querySelector<HTMLParagraphElement>("#update-status");
  const installButton = document.querySelector<HTMLButtonElement>("#install-update");
  if (!status || !installButton) return;

  const visible = shouldShowInstallButton(update);
  installButton.hidden = !visible;

  if (!update) {
    status.className = "status-message status-inline";
    status.textContent = "";
    return;
  }

  const statusClass =
    update.status === "check_failed"
      ? "status-error"
      : update.status === "update_available"
        ? "status-success"
        : "status-info";
  status.className = `status-message status-inline ${statusClass} show`;
  status.textContent = update.message;
}

function flashMessage(
  text: string,
  type: "success" | "error" | "info" = "info",
  targetId: "save-message" | "diagnostics-message" = "save-message",
): void {
  const message = document.querySelector<HTMLParagraphElement>(`#${targetId}`);
  if (!message) return;

  const extraClass = targetId === "diagnostics-message" ? " status-inline" : "";
  message.className = `status-message status-${type}${extraClass} show`;
  message.textContent = text;

  if (targetId === "save-message") {
    if (messageTimerSettings !== null) {
      window.clearTimeout(messageTimerSettings);
    }

    messageTimerSettings = window.setTimeout(() => {
      message.classList.remove("show");
    }, 3200);
    return;
  }

  if (messageTimerDiagnostics !== null) {
    window.clearTimeout(messageTimerDiagnostics);
  }

  messageTimerDiagnostics = window.setTimeout(() => {
    message.classList.remove("show");
  }, 3200);
}

async function loadState(forceFormSync = false): Promise<void> {
  const state = await invoke<AppStateResponse>("get_app_state");
  renderAll(state, { forceFormSync });
}

async function saveSettings(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  try {
    const saved = await invoke<AppSettings>("update_settings", { settings: readSettingsFromForm() });
    if (currentState) {
      currentState.settings = saved;
    }
    isSettingsFormDirty = false;
    flashMessage("Settings saved.", "success");
    await loadState(true);
  } catch (error) {
    captureUiError(error, {
      action: "save_settings_failed",
      errorTelemetryEnabled: currentState?.settings.error_telemetry_enabled ?? false,
      installationId: currentState?.installation_id ?? undefined,
    });
    flashMessage(`Failed to save settings: ${String(error)}`, "error");
  }
}

async function wireActions(): Promise<void> {
  document.querySelector("#settings-form")?.addEventListener("submit", (e) => {
    void saveSettings(e as SubmitEvent);
  });
  document.querySelector("#settings-form")?.addEventListener("input", () => {
    if (!isProgrammaticFormUpdate) {
      isSettingsFormDirty = true;
    }
  });
  document.querySelector("#settings-form")?.addEventListener("change", () => {
    if (!isProgrammaticFormUpdate) {
      isSettingsFormDirty = true;
    }
  });

  document.querySelector("#refresh-now")?.addEventListener("click", async () => {
    try {
      await invoke("manual_refresh");
      await loadState();
    } catch (error) {
      captureUiError(error, {
        action: "manual_refresh_failed",
        errorTelemetryEnabled: currentState?.settings.error_telemetry_enabled ?? false,
        installationId: currentState?.installation_id ?? undefined,
      });
      flashMessage(`Refresh failed: ${String(error)}`, "error");
    }
  });

  document.querySelector("#test-notification")?.addEventListener("click", async () => {
    try {
      await invoke("send_test_notification_cmd");
      flashMessage(
        "Notification dispatched. If this window is focused, macOS may suppress the banner.",
        "info",
      );
    } catch (error) {
      captureUiError(error, {
        action: "test_notification_failed",
        errorTelemetryEnabled: currentState?.settings.error_telemetry_enabled ?? false,
        installationId: currentState?.installation_id ?? undefined,
      });
      flashMessage(`Failed to dispatch test notification: ${String(error)}`, "error");
    }
  });

  document.querySelector("#check-updates")?.addEventListener("click", async () => {
    try {
      const response = await invoke<UpdateCheckResponse>("check_for_updates");
      renderUpdateState(response);
      flashMessage(response.message, response.status === "check_failed" ? "error" : "info");
      await loadState();
    } catch (error) {
      captureUiError(error, {
        action: "check_updates_failed",
        errorTelemetryEnabled: currentState?.settings.error_telemetry_enabled ?? false,
        installationId: currentState?.installation_id ?? undefined,
      });
      flashMessage(`Update check failed: ${String(error)}`, "error");
    }
  });

  document.querySelector("#install-update")?.addEventListener("click", async () => {
    try {
      const response = await invoke<string>("install_update");
      flashMessage(response, "success");
      await loadState();
    } catch (error) {
      captureUiError(error, {
        action: "install_update_failed",
        errorTelemetryEnabled: currentState?.settings.error_telemetry_enabled ?? false,
        installationId: currentState?.installation_id ?? undefined,
      });
      flashMessage(`Update install failed: ${String(error)}`, "error");
    }
  });

  if (IS_DEV_BUILD) {
    document.querySelector("#test-telemetry")?.addEventListener("click", async () => {
      try {
        const eventId = await invoke<string>("send_test_telemetry_event");
        flashMessage(`Telemetry event sent. Event ID: ${eventId}`, "success");
      } catch (error) {
        captureUiError(error, {
          action: "telemetry_test_failed",
          errorTelemetryEnabled: currentState?.settings.error_telemetry_enabled ?? false,
          installationId: currentState?.installation_id ?? undefined,
        });
        flashMessage(`Telemetry test failed: ${String(error)}`, "error");
      }
    });

    document.querySelector("#test-ui-error")?.addEventListener("click", () => {
      const testError = new Error("ResetPing UI error test");
      captureUiError(testError, {
        action: "manual_ui_error_test",
        errorTelemetryEnabled: currentState?.settings.error_telemetry_enabled ?? false,
        installationId: currentState?.installation_id ?? undefined,
      });
      flashMessage("UI error test sent.", "success");
    });
  }

  document.querySelector("#reset-endpoint")?.addEventListener("click", () => {
    const endpointInput = document.querySelector<HTMLInputElement>("#status_endpoint_url");
    if (!endpointInput) return;
    endpointInput.value = DEFAULT_ENDPOINT_URL;
    isSettingsFormDirty = true;
    flashMessage("Endpoint reset. Click Save Settings to persist.", "info");
  });

  document.querySelector("#copy-logs")?.addEventListener("click", async () => {
    try {
      const logs = await invoke<LogEntry[]>("get_recent_logs");
      const text = buildLogExport(logs);
      await writeText(text);
      flashMessage("Copied logs to clipboard.", "success", "diagnostics-message");
    } catch (error) {
      captureUiError(error, {
        action: "copy_logs_failed",
        errorTelemetryEnabled: currentState?.settings.error_telemetry_enabled ?? false,
        installationId: currentState?.installation_id ?? undefined,
      });
      flashMessage(`Failed to copy logs: ${String(error)}`, "error", "diagnostics-message");
    }
  });

  await listen<AppStateResponse>("state-updated", (event) => {
    renderAll(event.payload);
  });
}

void (async () => {
  await wireActions();
  await loadState(true);
})();
