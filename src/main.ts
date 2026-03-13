import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import "./styles.css";
import { buildLogExport, formatTimestamp } from "./utils/time";

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
  telemetry_enabled: boolean;
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
};

let currentState: AppStateResponse | null = null;
let messageTimerSettings: number | null = null;
let messageTimerDiagnostics: number | null = null;
let previousTransitionCount = 0;

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
        <label class="checkbox-row"><input type="checkbox" id="telemetry_enabled" /> Enable anonymous crash/error telemetry</label>

        <div class="form-actions">
          <button type="submit">Save Settings</button>
          <button id="check-updates" type="button">Check Updates</button>
          ${IS_DEV_BUILD ? '<button id="test-telemetry" type="button">Send Telemetry Test</button>' : ""}
          <button id="reset-endpoint" type="button">Reset Endpoint URL</button>
        </div>
      </form>
      <p id="save-message" class="status-message"></p>
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
  host.innerHTML = `
    <article class="card"><h3>Current State</h3><strong>${snapshot.last_known_state ?? "unknown"}</strong></article>
    <article class="card"><h3>Health</h3><strong>${health}</strong></article>
    <article class="card"><h3>Last Success</h3><strong>${formatTimestamp(snapshot.last_success_at)}</strong></article>
    <article class="card"><h3>Consecutive Failures</h3><strong>${snapshot.consecutive_failures}</strong></article>
  `;
}

function renderTransitions(transitions: TransitionEntry[]): void {
  const list = document.querySelector<HTMLUListElement>("#transition-list");
  if (!list) return;
  if (transitions.length === 0) {
    list.innerHTML = "<li class='muted'>No transitions yet.</li>";
    return;
  }

  list.innerHTML = transitions
    .slice()
    .reverse()
    .map((t) => `<li><strong>${t.from} -> ${t.to}</strong><br/><span>${formatTimestamp(t.detected_at)}</span></li>`)
    .join("");
}

function renderLogs(logs: LogEntry[]): void {
  const list = document.querySelector<HTMLUListElement>("#log-list");
  if (!list) return;
  if (logs.length === 0) {
    list.innerHTML = "<li class='muted'>No logs yet.</li>";
    return;
  }

  list.innerHTML = logs
    .slice()
    .reverse()
    .map((l) => `<li><strong>[${l.level.toUpperCase()}]</strong> ${l.message}<br/><span>${formatTimestamp(l.timestamp_ms)}</span></li>`)
    .join("");
}

function setFormValues(settings: AppSettings): void {
  (document.querySelector("#poll_interval_sec") as HTMLInputElement).value = String(settings.poll_interval_sec);
  (document.querySelector("#low_power_poll_interval_sec") as HTMLInputElement).value = String(settings.low_power_poll_interval_sec);
  (document.querySelector("#http_timeout_ms") as HTMLInputElement).value = String(settings.http_timeout_ms);
  (document.querySelector("#notification_policy") as HTMLSelectElement).value = settings.notification_policy;
  (document.querySelector("#notify_initial_state") as HTMLInputElement).checked = settings.notify_initial_state;
  (document.querySelector("#launch_at_login") as HTMLInputElement).checked = settings.launch_at_login;
  (document.querySelector("#update_checks_enabled") as HTMLInputElement).checked = settings.update_checks_enabled;
  (document.querySelector("#update_check_interval_hours") as HTMLInputElement).value = String(settings.update_check_interval_hours);
  (document.querySelector("#status_endpoint_url") as HTMLInputElement).value = settings.status_endpoint_url;
  (document.querySelector("#telemetry_enabled") as HTMLInputElement).checked = settings.telemetry_enabled;
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
    telemetry_enabled: (document.querySelector("#telemetry_enabled") as HTMLInputElement).checked,
  };
}

function renderAll(state: AppStateResponse): void {
  currentState = state;
  setStatusCards(state.snapshot, state.health);
  setFormValues(state.settings);
  renderTransitions(state.transitions);
  renderLogs(state.logs);

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

async function loadState(): Promise<void> {
  const state = await invoke<AppStateResponse>("get_app_state");
  renderAll(state);
}

async function saveSettings(event: SubmitEvent): Promise<void> {
  event.preventDefault();
  try {
    const saved = await invoke<AppSettings>("update_settings", { settings: readSettingsFromForm() });
    if (currentState) {
      currentState.settings = saved;
    }
    flashMessage("Settings saved.", "success");
    await loadState();
  } catch (error) {
    flashMessage(`Failed to save settings: ${String(error)}`, "error");
  }
}

async function wireActions(): Promise<void> {
  document.querySelector("#settings-form")?.addEventListener("submit", (e) => {
    void saveSettings(e as SubmitEvent);
  });

  document.querySelector("#refresh-now")?.addEventListener("click", async () => {
    await invoke("manual_refresh");
    await loadState();
  });

  document.querySelector("#test-notification")?.addEventListener("click", async () => {
    try {
      await invoke("send_test_notification_cmd");
      flashMessage(
        "Notification dispatched. If this window is focused, macOS may suppress the banner.",
        "info",
      );
    } catch (error) {
      flashMessage(`Failed to dispatch test notification: ${String(error)}`, "error");
    }
  });

  document.querySelector("#check-updates")?.addEventListener("click", async () => {
    const response = await invoke<{ result: string }>("check_for_updates");
    flashMessage(response.result, "info");
    await loadState();
  });

  if (IS_DEV_BUILD) {
    document.querySelector("#test-telemetry")?.addEventListener("click", async () => {
      try {
        const eventId = await invoke<string>("send_test_telemetry_event");
        flashMessage(`Telemetry event sent. Event ID: ${eventId}`, "success");
      } catch (error) {
        flashMessage(`Telemetry test failed: ${String(error)}`, "error");
      }
    });
  }

  document.querySelector("#reset-endpoint")?.addEventListener("click", () => {
    const endpointInput = document.querySelector<HTMLInputElement>("#status_endpoint_url");
    if (!endpointInput) return;
    endpointInput.value = DEFAULT_ENDPOINT_URL;
    flashMessage("Endpoint reset. Click Save Settings to persist.", "info");
  });

  document.querySelector("#copy-logs")?.addEventListener("click", async () => {
    try {
      const logs = await invoke<LogEntry[]>("get_recent_logs");
      const text = buildLogExport(logs);
      await writeText(text);
      flashMessage("Copied logs to clipboard.", "success", "diagnostics-message");
    } catch (error) {
      flashMessage(`Failed to copy logs: ${String(error)}`, "error", "diagnostics-message");
    }
  });

  await listen<AppStateResponse>("state-updated", (event) => {
    renderAll(event.payload);
  });
}

void (async () => {
  await wireActions();
  await loadState();
})();
