// Read-only spectator page policy. Rust validates the stream URL again before opening it.

/** Message type a configured parent page sends to switch the spectated stream. */
export const STREAM_MESSAGE = "cinnabar-spectate";
/** Message type this page sends its parent for each state or error notice. */
export const NOTICE_MESSAGE = "cinnabar-spectator-notice";

/** Accepts an HTTPS URL, or plain HTTP on a loopback host for local development. */
function secure(url) {
  const loopback = ["localhost", "127.0.0.1", "[::1]"].includes(url.hostname);
  return url.protocol === "https:" || (url.protocol === "http:" && loopback);
}

/**
 * Reads `assets` (a directory ending in /), `stream` and the optional `parent` origin from a
 * page query string. Throws a presentable error for missing or insecure values.
 */
export function spectatorParams(search) {
  const query = new URLSearchParams(search);
  const assets = new URL(query.get("assets") ?? "", "https://invalid.invalid/");
  if (!query.get("assets") || !secure(assets) || !assets.pathname.endsWith("/")) {
    throw new Error("Use an HTTPS asset directory ending in /");
  }
  const stream = query.get("stream");
  if (!stream || !secure(new URL(stream, "https://invalid.invalid/")) || !URL.canParse(stream)) {
    throw new Error("Use an HTTPS spectator stream URL");
  }
  let parent = null;
  if (query.get("parent")) {
    const origin = new URL(query.get("parent"));
    if (!secure(origin)) throw new Error("Use an HTTPS parent origin");
    parent = origin.origin;
  }
  return { assets, stream, parent };
}

/** Returns the validated `parent` origin for notices, or null, even when other values are invalid. */
export function noticeOrigin(search) {
  try {
    const value = new URLSearchParams(search).get("parent");
    if (!value) return null;
    const origin = new URL(value);
    return secure(origin) ? origin.origin : null;
  } catch { return null; }
}

/**
 * Returns the stream a message asks for, or null unless it came from the configured parent
 * origin and the embedding window itself (`parentWindow`, absent for a top-level page).
 */
export function requestedStream(event, parent, parentWindow) {
  if (!parent || !parentWindow || event.origin !== parent || event.source !== parentWindow) return null;
  const data = event.data;
  if (!data || data.type !== STREAM_MESSAGE || typeof data.stream !== "string") return null;
  return data.stream;
}
