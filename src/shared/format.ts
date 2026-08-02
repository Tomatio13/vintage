export function formatFileSize(bytes: number) {
  const safeBytes = Math.max(0, bytes);
  if (safeBytes < 1_024) return `${safeBytes} B`;

  const units = ["KB", "MB", "GB", "TB"];
  let value = safeBytes / 1_024;
  let unitIndex = 0;

  while (value >= 1_024 && unitIndex < units.length - 1) {
    value /= 1_024;
    unitIndex += 1;
  }

  return `${value >= 10 ? value.toFixed(0) : value.toFixed(1)} ${units[unitIndex]}`;
}

export function formatTokenCount(value: number) {
  return new Intl.NumberFormat("en", {
    notation: value >= 10_000 ? "compact" : "standard",
  }).format(value);
}

export function cleanVersion(version: string | null) {
  return version?.replace(/^grok\s+/, "") ?? "not detected";
}

export function formatUsedPercent(value: number) {
  if (!Number.isFinite(value)) return "—";
  const rounded = Math.round(value * 10) / 10;
  if (Math.abs(rounded - Math.round(rounded)) < 0.05) {
    return `${Math.round(rounded)}%`;
  }
  return `${rounded.toFixed(1)}%`;
}

export function formatUsageReset(iso: string) {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "Reset time unknown";
  if (date.getTime() <= Date.now()) return "Resets soon";
  const scheduled = new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
  return `Resets ${scheduled}`;
}
