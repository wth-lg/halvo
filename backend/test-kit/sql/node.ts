import type { ProviderPort } from "../../src/billing/port.ts";
import { type Host, ownTables } from "../../src/do/entries.ts";
import type { Ms, Sql, Step } from "../../src/types.ts";

type Row = Record<string, string | number | null>;
interface Statement {
  all(...bindings: (string | number | null)[]): unknown[];
}
interface Database {
  prepare(query: string): Statement;
  exec(query: string): void;
  close(): void;
}
interface NodeSqlite {
  DatabaseSync: new (path: string) => Database;
}

function nodeSqlite(): NodeSqlite {
  const node = globalThis as unknown as { process: { getBuiltinModule(id: string): unknown } };
  return node.process.getBuiltinModule("node:sqlite") as NodeSqlite;
}

function semicolonRefused(): never {
  const error = new Error("one call of exec holds one statement and no semicolon");
  error.name = "SqlSemicolonError";
  throw error;
}

export function openSql(): { step: Step; dropAll(): void; close(): void } {
  const { DatabaseSync } = nodeSqlite();
  const db = new DatabaseSync(":memory:");
  const sql: Sql = {
    exec(query, ...bindings) {
      if (query.includes(";")) semicolonRefused();
      return db
        .prepare(query)
        .all(...bindings)
        .map((row) => ({ ...(row as Row) }));
    },
  };
  const step: Step = (run) => {
    db.exec("BEGIN");
    try {
      const value = run(sql);
      db.exec("COMMIT");
      return value;
    } catch (error) {
      db.exec("ROLLBACK");
      throw error;
    }
  };
  return {
    step,
    dropAll() {
      for (const name of ownTables(sql)) db.exec(`DROP TABLE "${name.replaceAll('"', '""')}"`);
    },
    close() {
      db.close();
    },
  };
}

export function makeTestHost(o: { port: ProviderPort; config?: Partial<Host["config"]>; clock?: () => Ms }): {
  host: Host;
  alarmAt(): Ms | null;
  close(): void;
} {
  const db = openSql();
  let alarm: Ms | null = null;
  const host: Host = {
    step: db.step,
    clock: o.clock ?? (() => 0),
    setAlarm(at) {
      alarm = at;
    },
    async getAlarm() {
      return alarm;
    },
    async deleteAll() {
      db.dropAll();
      alarm = null;
    },
    port: o.port,
    config: {
      holdLifeMs: 3000,
      intervalSeconds: 3600,
      marginSeconds: 300,
      publicOrigin: "http://halvo.test",
      testClock: true,
      ...o.config,
    },
  };
  return { host, alarmAt: () => alarm, close: () => db.close() };
}
