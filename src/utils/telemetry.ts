import * as Sentry from "@sentry/browser";

type FrontendTelemetryConfig = {
  dsn?: string;
  installationId: string;
  errorTelemetryEnabled: boolean;
};

let initialized = false;

export function initFrontendTelemetry(config: FrontendTelemetryConfig): void {
  if (initialized || !config.errorTelemetryEnabled || !config.dsn) {
    return;
  }

  Sentry.init({
    dsn: config.dsn,
    release: `resetping-frontend@${__APP_VERSION__}`,
    environment: import.meta.env.DEV ? "development" : "production",
    sendDefaultPii: false,
  });

  Sentry.setTag("component", "ui");
  Sentry.setTag("platform", "tauri");
  Sentry.setTag("app_version", __APP_VERSION__);
  Sentry.setTag("build_channel", import.meta.env.DEV ? "debug" : "release");
  Sentry.setTag("installation_id", config.installationId);
  initialized = true;
}

export function captureUiError(
  error: unknown,
  options: { action: string; errorTelemetryEnabled: boolean; installationId?: string },
): void {
  if (!initialized || !options.errorTelemetryEnabled) {
    return;
  }

  const resolved = error instanceof Error ? error : new Error(String(error));
  Sentry.withScope((scope) => {
    scope.setTag("event_type", "error");
    scope.setTag("component", "ui");
    scope.setTag("error_kind", options.action);
    if (options.installationId) {
      scope.setTag("installation_id", options.installationId);
    }
    Sentry.captureException(resolved);
  });
}
