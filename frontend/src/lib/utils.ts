import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

/** Merge Tailwind class strings safely. */
export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Format an ISO string or unix-seconds timestamp to a short time string. */
export function fmtTime(ts: string | number | undefined): string {
  if (!ts) return "—";
  const d = typeof ts === "number" ? new Date(ts * 1000) : new Date(ts);
  return isNaN(d.getTime()) ? String(ts) : d.toLocaleTimeString();
}

/** Format an ISO string or unix-seconds timestamp to a full date-time string. */
export function fmtDateTime(ts: string | number | undefined): string {
  if (!ts) return "—";
  const d = typeof ts === "number" ? new Date(ts * 1000) : new Date(ts);
  return isNaN(d.getTime()) ? String(ts) : d.toLocaleString();
}

/** Truncate a string to `maxLen` characters, appending '…' if truncated. */
export function truncate(s: string, maxLen: number): string {
  return s.length > maxLen ? s.slice(0, maxLen) + "…" : s;
}

/** Copy text to clipboard and return true on success. */
export async function copyToClipboard(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}
