export type UpdateCheckStatus =
  | "up_to_date"
  | "update_available"
  | "unsupported_platform"
  | "check_failed";

export type UpdateCheckResponse = {
  checked_at: number;
  status: UpdateCheckStatus;
  version: string | null;
  current_version: string | null;
  notes: string | null;
  install_ready: boolean;
  message: string;
};

export function shouldShowInstallButton(update: UpdateCheckResponse | null): boolean {
  return Boolean(update && update.status === "update_available" && update.install_ready);
}
