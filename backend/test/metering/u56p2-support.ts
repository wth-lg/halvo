import { makeAccounts } from "../../src/accounts/index.ts";
import { makeEntries } from "../../src/do/entries.ts";
import { makeLedger } from "../../src/ledger/index.ts";
import { makeMetering } from "../../src/metering/index.ts";
import type { Ms } from "../../src/types.ts";
import type { CancelFault } from "../../test-kit/fake-meter/port.ts";
import { ACCOUNT, HASH, type ListedCustomer, quiet, START, subscriptionEvent, type World, world } from "./world.ts";

export const MINUTE = 60_000;
export const CAP_MS = 3_600_000;

export function cancels(w: World): (string | number | null)[][] {
  return w.calls.flatMap((each) => (each.kind === "cancel" ? [[each.subscriptionId, each.customerId, each.answer, ...(each.cancelled === undefined ? [] : [each.cancelled])]] : []));
}

export function periodsAfter(w: World, mark: Ms, until: Ms): number {
  return w.meter.periods(ACCOUNT, Math.floor(until / 1000)).filter((period) => period.start * 1000 > mark).length;
}

export function statusOf(w: World, id: string): string | null {
  return w.meter.plan(id)?.status ?? null;
}

export async function skeleton(w: World): Promise<Record<string, string | number | null> | null> {
  return (await w.tables()).skeleton?.[0] ?? null;
}

export async function callRecord(w: World): Promise<Record<string, string | number | null>[]> {
  return (await w.tables()).cancel_call ?? [];
}

export async function holds(w: World): Promise<boolean> {
  return (await w.entries().testInspect()).result === "snapshot";
}

export function destroy(w: World) {
  return quiet(w.lines, () => w.entries().destroy(HASH));
}

export function events(w: World): string[] {
  return w.lines.map((line) => String(JSON.parse(line).event));
}

export async function subscribed(w: World, id = "sub_1", at: Ms = START + 1000): Promise<void> {
  await w.entries().webhookEvent(subscriptionEvent({ eventId: `evt_${id}`, providerTime: at, subscriptionId: id }));
}

export function held(): { fault: CancelFault; release(): void } {
  let release: () => void = () => {};
  const waiting = new Promise<void>((resolve) => {
    release = resolve;
  });
  return { fault: { kind: "during", run: () => waiting }, release: () => release() };
}

export function started(w: World): (string | null)[] {
  const ids: (string | null)[] = [];
  const inner = w.host.port.cancelSubscription.bind(w.host.port);
  w.host.port.cancelSubscription = (id, customerId, settled) => {
    ids.push(id);
    return inner(id, customerId, settled);
  };
  return ids;
}

export function secondInstance(w: World) {
  return makeEntries(w.host, {
    accounts: makeAccounts(),
    ledger: makeLedger({ holdLifeMs: w.host.config.holdLifeMs }),
    metering: makeMetering({ intervalSeconds: w.host.config.intervalSeconds, marginSeconds: w.host.config.marginSeconds }),
  });
}

export async function listed(customer: ListedCustomer, o: Parameters<typeof world>[0] = {}): Promise<World> {
  return world({ testClock: false, ...o, listed: customer });
}

export const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

export async function alarmAt(w: World, at: Ms): Promise<void> {
  w.at(at);
  await quiet(w.lines, () => w.entries().alarm());
}
