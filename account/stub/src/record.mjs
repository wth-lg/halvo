import { appendFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** Thrown when the directory that the caller names for the record is missing, not a directory, or not empty. */
export class RecordError extends Error {
  constructor(message) {
    super(message);
    this.name = "RecordError";
  }
}

const number = (n) => String(n).padStart(6, "0");

/**
 * Opens a record in `dir`, an empty directory that the caller made. The record keeps, as files of `dir`:
 * `connection-NNNNNN.raw`, every byte that a connection received, in order; `request-NNNNNN.body`, the body bytes
 * of request NNNNNN as the stub read them; and `requests.jsonl`, one line for each request in the order of its answer,
 * `{seq, connection, method, target, headers, body, bytes, status, result}`, or `{seq, connection, error}` for a request
 * that the HTTP parser refused. `connection(socket)` starts the raw file of a new connection and returns its number;
 * `request(...)` and `refused(...)` write one line each before the stub answers.
 */
export function openRecord(dir) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    throw new RecordError("the record directory is missing or not a directory");
  }
  if (entries.length > 0) throw new RecordError("the record directory is not empty");
  const jsonl = join(dir, "requests.jsonl");
  writeFileSync(jsonl, "", { flag: "wx" });
  let connections = 0;
  let seq = 0;
  const line = (entry) => appendFileSync(jsonl, `${JSON.stringify(entry)}\n`);

  return {
    connection(socket) {
      connections += 1;
      const raw = join(dir, `connection-${number(connections)}.raw`);
      writeFileSync(raw, "", { flag: "wx" });
      socket.prependListener("data", (chunk) => appendFileSync(raw, chunk));
      return connections;
    },
    request({ connection, method, target, headers, body, status, result }) {
      seq += 1;
      const name = `request-${number(seq)}.body`;
      writeFileSync(join(dir, name), body, { flag: "wx" });
      line({ seq, connection, method, target, headers, body: name, bytes: body.length, status, result });
    },
    refused({ connection, error }) {
      seq += 1;
      line({ seq, connection, error });
    },
  };
}
