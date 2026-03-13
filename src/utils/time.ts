export function formatTimestamp(ms: number | null): string {
  if (!ms) {
    return "Never";
  }

  return new Date(ms).toLocaleString();
}

export function buildLogExport(
  logs: Array<{ timestamp_ms: number; level: string; message: string }>,
): string {
  return logs
    .map((l) => `${new Date(l.timestamp_ms).toISOString()} [${l.level.toUpperCase()}] ${l.message}`)
    .join("\n");
}
