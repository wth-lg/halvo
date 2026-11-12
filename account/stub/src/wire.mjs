/** The stub keeps the first this many bytes of a body, and reads the rest without keeping it. */
export const BODY_CAP = 1048576;
/** The most bytes that a request head, or the trailer section of a chunked body, may take. */
export const HEAD_MAX = 65536;
const LINE_MAX = 4096;

const TOKEN = "[!#$%&'*+.^_`|~0-9A-Za-z-]+";
const REQUEST_LINE = new RegExp(`^(${TOKEN}) ([\\x21-\\xff]+) HTTP/1\\.([01])$`);
const FIELD_LINE = new RegExp(`^(${TOKEN}):[ \\t]*([^\\r\\n\\x00]*?)[ \\t]*$`);
const CHUNK_SIZE = /^([0-9A-Fa-f]{1,13})(?:[ \t]*;[^\r\n\x00]*)?$/;
const CRLF = Buffer.from("\r\n");

function fields(lines) {
  const out = [];
  for (const line of lines) {
    const match = FIELD_LINE.exec(line);
    if (match === null) return null;
    out.push([match[1], match[2]]);
  }
  return out;
}

const values = (list, name) => list.filter(([key]) => key.toLowerCase() === name).map(([, value]) => value);

/**
 * Makes a reader of HTTP/1.1 requests from the bytes of one connection, in order. `feed(chunk)` takes the next bytes
 * and `end()` says that the peer sent no more. For each whole request it calls `request(message)` with
 * `{method, target, version, headers, trailers, body}`, the fields as [name, value] pairs in the order received, each
 * byte as one character, and the first BODY_CAP bytes of the body; `request` returns true to read the next request
 * and false to stop. For bytes that are no request it calls `malformed(cause)` once and reads nothing more; the cause
 * is one of `head-too-large`, `request-line`, `field-line`, `framing`, `content-length`, `transfer-encoding`,
 * `chunk`, `incomplete`.
 */
export function makeReader({ request, malformed }) {
  let buffer = Buffer.alloc(0);
  let state = "head";
  let message = null;
  let remaining = 0;
  let kept = [];
  let keptBytes = 0;
  let trailerBytes = 0;

  const fail = (cause) => {
    state = "done";
    malformed(cause);
  };
  const stop = (cause) => {
    fail(cause);
    return false;
  };
  const keep = (bytes) => {
    const part = bytes.subarray(0, Math.max(0, BODY_CAP - keptBytes));
    if (part.length > 0) kept.push(part);
    keptBytes += part.length;
  };
  const finish = () => {
    const whole = { ...message, body: Buffer.concat(kept) };
    message = null;
    kept = [];
    keptBytes = 0;
    state = "head";
    if (!request(whole)) state = "done";
  };
  const take = (n) => {
    const part = buffer.subarray(0, n);
    buffer = buffer.subarray(part.length);
    return part;
  };

  function head() {
    while (buffer.subarray(0, 2).equals(CRLF)) take(2);
    const end = buffer.indexOf("\r\n\r\n");
    if (end < 0) {
      if (buffer.length > HEAD_MAX + 3) fail("head-too-large");
      return false;
    }
    if (end > HEAD_MAX) return stop("head-too-large");
    const [first, ...rest] = take(end + 4).subarray(0, end).toString("latin1").split("\r\n");
    const line = REQUEST_LINE.exec(first);
    if (line === null) return stop("request-line");
    const headers = fields(rest);
    if (headers === null) return stop("field-line");
    message = { method: line[1], target: line[2], version: `1.${line[3]}`, headers, trailers: [] };
    const encodings = values(headers, "transfer-encoding");
    const lengths = values(headers, "content-length");
    if (encodings.length > 0 && lengths.length > 0) return stop("framing");
    if (encodings.length > 0) {
      if (encodings.join(",").trim().toLowerCase() !== "chunked") return stop("transfer-encoding");
      state = "size";
      return true;
    }
    if (lengths.some((value) => !/^[0-9]{1,15}$/.test(value) || value !== lengths[0])) return stop("content-length");
    remaining = lengths.length > 0 ? Number(lengths[0]) : 0;
    if (remaining === 0) finish();
    else state = "length";
    return true;
  }

  function line(limit, cause) {
    const end = buffer.indexOf("\r\n");
    if (end < 0) {
      if (buffer.length > limit) fail(cause);
      return null;
    }
    return take(end + 2).subarray(0, end).toString("latin1");
  }

  function step() {
    for (;;) {
      if (state === "head") {
        if (!head()) return;
      } else if (state === "length" || state === "data") {
        if (buffer.length === 0) return;
        const part = take(remaining);
        keep(part);
        remaining -= part.length;
        if (remaining > 0) continue;
        if (state === "length") finish();
        else state = "data-end";
      } else if (state === "size") {
        const text = line(LINE_MAX, "chunk");
        if (text === null) return;
        const match = CHUNK_SIZE.exec(text);
        if (match === null) return fail("chunk");
        remaining = parseInt(match[1], 16);
        trailerBytes = 0;
        state = remaining === 0 ? "trailers" : "data";
      } else if (state === "data-end") {
        if (buffer.length < 2) return;
        if (!take(2).equals(CRLF)) return fail("chunk");
        state = "size";
      } else if (state === "trailers") {
        const text = line(HEAD_MAX, "head-too-large");
        if (text === null) return;
        trailerBytes += text.length + 2;
        if (trailerBytes > HEAD_MAX) return fail("head-too-large");
        if (text === "") {
          finish();
          continue;
        }
        const field = fields([text]);
        if (field === null) return fail("field-line");
        message.trailers.push(field[0]);
      } else {
        return;
      }
    }
  }

  return {
    feed(chunk) {
      if (state === "done") return;
      buffer = buffer.length === 0 ? chunk : Buffer.concat([buffer, chunk]);
      step();
    },
    end() {
      if (state === "done") return;
      if (state !== "head" || buffer.length > 0) fail("incomplete");
      state = "done";
    },
  };
}
