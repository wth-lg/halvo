import { closeSync, openSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** Thrown when the directory that the caller names for the record is missing, not a directory, or not empty. */
export class RecordError extends Error {
  constructor(message) {
    super(message);
    this.name = "RecordError";
  }
}

const number = (n) => String(n).padStart(6, "0");

const open = (path) => {
  try {
    return openSync(path, "wx");
  } catch {
    return null;
  }
};

/**
 * Opens a record in `dir`, an empty directory that the caller made. The record keeps, as files of `dir`:
 * `connection-NNNNNN.raw`, every byte that a connection received, in order; `request-NNNNNN.body`, the body bytes
 * that the stub kept of request NNNNNN; and `requests.jsonl`, one line for each request in the order of its answer,
 * `{seq, connection, method, target, headers, trailers, body, bytes, status, result}`, or `{seq, connection, error}` for bytes
 * that are no request. requests.jsonl and the raw file of each open connection stay open, so that a line or the bytes
 * of a connection need no new file. `first()` runs after `dir` is checked and before the record writes its first file,
 * so that what it throws leaves `dir` as it was.
 * `connection(socket)` numbers a new connection and opens its raw file, and returns `{connection, opened}`, with
 * `opened` false when the open failed, EMFILE among the causes. `request({..., judge})` opens the body file of the next
 * request and writes the body, then calls `judge()` for the answer and writes the line; when the body file cannot be
 * opened, it writes the line `body-open`, does not call `judge` and returns null. `refused(...)` writes one line. Each
 * line is written before the stub answers. `close()` closes every file that the record holds open.
 */
export function openRecord(dir, first = () => {}) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    throw new RecordError("the record directory is missing or not a directory");
  }
  if (entries.length > 0) throw new RecordError("the record directory is not empty");
  first();
  const jsonl = openSync(join(dir, "requests.jsonl"), "wx");
  const raws = new Set();
  let connections = 0;
  let seq = 0;
  const line = (entry) => writeFileSync(jsonl, `${JSON.stringify(entry)}\n`);
  const shut = (fd) => {
    if (raws.delete(fd)) closeSync(fd);
  };

  return {
    connection(socket) {
      connections += 1;
      const fd = open(join(dir, `connection-${number(connections)}.raw`));
      if (fd === null) return { connection: connections, opened: false };
      raws.add(fd);
      socket.on("data", (chunk) => writeFileSync(fd, chunk));
      socket.once("close", () => shut(fd));
      return { connection: connections, opened: true };
    },
    request({ connection, method, target, headers, trailers, body, judge }) {
      seq += 1;
      const name = `request-${number(seq)}.body`;
      const fd = open(join(dir, name));
      if (fd === null) {
        line({ seq, connection, error: "body-open" });
        return null;
      }
      try {
        writeFileSync(fd, body);
      } finally {
        closeSync(fd);
      }
      const answer = judge();
      line({ seq, connection, method, target, headers, trailers, body: name, bytes: body.length, status: answer.status, result: answer.result });
      return answer;
    },
    refused({ connection, error }) {
      seq += 1;
      line({ seq, connection, error });
    },
    close() {
      for (const fd of [...raws]) shut(fd);
      closeSync(jsonl);
    },
  };
}
