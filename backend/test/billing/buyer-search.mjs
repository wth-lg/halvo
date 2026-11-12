import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdtempSync, readdirSync, readFileSync, statSync } from "node:fs";
import { basename, join, relative } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { gunzipSync } from "node:zlib";

export function allFiles(dir) {
  const found = [];
  if (!existsSync(dir)) return found;
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    const info = statSync(path);
    if (info.isDirectory()) found.push(...allFiles(path));
    else if (info.isFile()) found.push(path);
  }
  return found;
}

function sleepSync(ms) {
  Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
}

export function readable(path, scratch) {
  if (existsSync(`${path}-shm`)) return { path, readOnly: true };
  const dir = mkdtempSync(join(scratch, "subscription-db."));
  const copy = join(dir, "copy.sqlite");
  copyFileSync(path, copy);
  if (existsSync(`${path}-wal`)) copyFileSync(`${path}-wal`, `${copy}-wal`);
  return { path: copy, readOnly: false };
}

export function readTables(path, scratch, keep, { bigInts = false } = {}) {
  for (let tries = 0; ; tries += 1) {
    let db;
    try {
      const source = readable(path, scratch);
      db = new DatabaseSync(source.path, { readOnly: source.readOnly });
      const names = db
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .all()
        .map((row) => String(row.name))
        .filter(keep);
      const tables = {};
      for (const name of names) {
        const statement = db.prepare(`SELECT * FROM "${name.replaceAll('"', '""')}"`);
        if (bigInts) statement.setReadBigInts(true);
        tables[name] = statement.all().map((row) => ({ ...row }));
      }
      return tables;
    } catch (error) {
      if (tries >= 50 || !/locked|busy/i.test(String(error?.message))) throw error;
      sleepSync(100);
    } finally {
      db?.close();
    }
  }
}

function blobForms(bytes) {
  const raw = Buffer.from(bytes);
  const forms = [raw, Buffer.from(raw.toString("latin1"), "base64")];
  if (raw[0] === 0x1f && raw[1] === 0x8b) {
    try {
      forms.push(gunzipSync(raw));
    } catch {}
  }
  return forms;
}

function valuesIn(texts, buffers, values) {
  return values.filter((value) => texts.some((text) => text.includes(value)) || buffers.some((buffer) => buffer.includes(Buffer.from(value, "utf8"))));
}

export function searchBuyerValues(dirs, values, scratch) {
  const found = new Set();
  const lines = [];
  const record = (label, place, hits) => {
    for (const value of hits) found.add(value);
    if (hits.length > 0) lines.push(`buyer values in ${label}, ${place}: ${hits.length}`);
  };
  for (const dir of dirs) {
    for (const path of allFiles(dir)) {
      const label = join(basename(dir), relative(dir, path));
      record(label, "raw bytes", valuesIn([], [readFileSync(path)], values));
      if (!path.endsWith(".sqlite")) continue;
      const source = readable(path, scratch);
      const dump = spawnSync("sqlite3", [...(source.readOnly ? ["-readonly"] : []), source.path, ".dump"], {
        encoding: "utf8",
        maxBuffer: 64 * 1024 * 1024,
      });
      if (dump.status !== 0) throw new Error(`sqlite3 could not read ${label}: ${dump.stderr.trim()}`);
      record(label, "sqlite3 .dump", valuesIn([dump.stdout], [], values));
      const tables = readTables(path, scratch, () => true, { bigInts: true });
      for (const [name, rows] of Object.entries(tables)) {
        const texts = [];
        const buffers = [];
        for (const row of rows) {
          for (const cell of Object.values(row)) {
            if (typeof cell === "string") texts.push(cell);
            else if (cell instanceof Uint8Array) buffers.push(...blobForms(cell));
          }
        }
        record(label, `table ${name}`, valuesIn(texts, buffers, values));
      }
    }
  }
  lines.push(`buyer values found in the state directories: ${found.size}`);
  return { count: found.size, lines };
}
