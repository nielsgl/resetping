import { describe, expect, it } from "vitest";
import { shouldShowInstallButton, type UpdateCheckResponse } from "./updater";

function makeUpdate(overrides: Partial<UpdateCheckResponse> = {}): UpdateCheckResponse {
  return {
    checked_at: 1,
    status: "up_to_date",
    version: null,
    current_version: "0.1.0",
    notes: null,
    install_ready: false,
    message: "ok",
    ...overrides,
  };
}

describe("shouldShowInstallButton", () => {
  it("shows install only when update is available and marked install-ready", () => {
    expect(
      shouldShowInstallButton(
        makeUpdate({
          status: "update_available",
          install_ready: true,
          version: "0.2.0",
        }),
      ),
    ).toBe(true);
    expect(
      shouldShowInstallButton(
        makeUpdate({
          status: "update_available",
          install_ready: false,
          version: "0.2.0",
        }),
      ),
    ).toBe(false);
    expect(shouldShowInstallButton(makeUpdate({ status: "up_to_date" }))).toBe(false);
    expect(shouldShowInstallButton(makeUpdate({ status: "check_failed" }))).toBe(false);
    expect(shouldShowInstallButton(makeUpdate({ status: "unsupported_platform" }))).toBe(false);
  });
});
