import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { errorName, log, numberVariable } from "../../src/log.ts";

let lines: string[] = [];
beforeEach(() => {
  lines = [];
  vi.spyOn(console, "log").mockImplementation((text: unknown) => {
    lines.push(String(text));
  });
});
afterEach(() => {
  vi.restoreAllMocks();
});

test("log writes the fields it was given in the order of LogFields and drops every other key", () => {
  log({ variable: "X", error: "E", status: 400, route: "status", event: "rejected" });
  log({ event: "handler-error", route: undefined, status: 500 });
  log({ event: "pending-dropped", body: "secret" } as unknown as Parameters<typeof log>[0]);
  expect(lines).toEqual([
    '{"event":"rejected","route":"status","status":400,"error":"E","variable":"X"}',
    '{"event":"handler-error","status":500}',
    '{"event":"pending-dropped"}',
  ]);
});

test("numberVariable takes a positive whole number in decimal digits and nothing else", () => {
  expect(numberVariable("HOLD_LIFE_MS", "3000")).toBe(3000);
  expect(numberVariable("HOLD_LIFE_MS", "999999999999999")).toBe(999_999_999_999_999);
  for (const bad of [undefined, "", "0", "-1", "01", "1.5", "1e3", " 3", "3 ", "1000000000000000", "x"]) {
    lines = [];
    let thrown: unknown = null;
    try {
      numberVariable("HOLD_LIFE_MS", bad);
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(Error);
    expect((thrown as Error).name).toBe("HOLD_LIFE_MS");
    expect((thrown as Error).message).toBe("a number variable is not a positive whole number in decimal digits");
    expect(lines).toEqual(['{"event":"bad-variable","variable":"HOLD_LIFE_MS"}']);
  }
});

test("errorName passes a name that matches ^[A-Za-z]{0,59}Error$ and gives Error for every other caught value", () => {
  const named = (name: unknown) => Object.defineProperty(new Error("never logged"), "name", { value: name });
  expect(errorName(new TypeError("never logged"))).toBe("TypeError");
  for (const kept of ["SqlSemicolonError", "NotBuiltError", "ContractBrokenError"]) expect(errorName(named(kept))).toBe(kept);
  expect(errorName(named(`${"A".repeat(59)}Error`))).toBe(`${"A".repeat(59)}Error`);
  expect(errorName(named(`${"A".repeat(60)}Error`))).toBe("Error");
  expect(errorName(named("3b8f1c2a-7d4e-4f6a-9b0c-1d2e3f4a5b6c"))).toBe("Error");
  expect(errorName(named("boom"))).toBe("Error");
  expect(errorName(named(42))).toBe("Error");
  const getterThrows = Object.defineProperty(new Error("never logged"), "name", {
    get() {
      throw new Error("never logged");
    },
  });
  expect(errorName(getterThrows)).toBe("Error");
  for (const value of [null, undefined, "TypeError", 7]) expect(errorName(value)).toBe("Error");
  let variable: unknown = null;
  try {
    numberVariable("HOLD_LIFE_MS", "x");
  } catch (error) {
    variable = error;
  }
  expect(errorName(variable)).toBe("Error");
});
