import { readFileSync } from "node:fs";

const IPV6_REACHABILITY_CHECK_ADDRESS = "[2001:4860:4860::8888]:443";
const PROBE_EVENTS = ["UDP_CONNECT", "SOCKET_CONNECT"];
const DATAGRAM_EVENTS = ["UDP_BYTES_SENT", "UDP_SEND_ERROR"];
const ERR_ACCESS_DENIED = -10;
const ADDRESS_KEYS = ["address", "local_address", "remote_address", "peer_address"];
const LOOPBACK = /^(127\.\d+\.\d+\.\d+|\[::1\]|\[::ffff:127\.\d+\.\d+\.\d+\])(:\d+)?$/;
const ANSWERED_BY_CHROMIUM_ITSELF = /^(.*\.)?localhost\.?$/i;
const MOST_HOPS_FROM_A_PROBE_SOCKET_TO_ITS_CHECK = 2;

function nameSentToResolver(host) {
  const name = host.replace(/^[a-z][a-z0-9+.-]*:\/\//i, "").replace(/:\d+$/, "");
  if (name.startsWith("[") || /^\d+\.\d+\.\d+\.\d+$/.test(name) || ANSWERED_BY_CHROMIUM_ITSELF.test(name)) return null;
  return name;
}

const socketsThatEachShowAsOneSourceOrTwo = (eventsByType) => Math.max(...Object.values(eventsByType));

function attemptsByAddress(kinds) {
  const countOfMostFrequentKind = new Map();
  for (const { count, address } of kinds.values()) {
    countOfMostFrequentKind.set(address, Math.max(countOfMostFrequentKind.get(address) ?? 0, count));
  }
  return countOfMostFrequentKind;
}

export function countOutside(log) {
  const eventName = new Map(Object.entries(log.constants.logEventTypes).map(([name, id]) => [id, name]));
  const sources = new Map();
  const madeBy = new Map();
  const uncachedIpv6ChecksBySource = new Map();
  const kinds = new Map();
  let systemProxy = false;
  const note = (what, address) => {
    const entry = kinds.get(what) ?? { count: 0, address };
    entry.count += 1;
    kinds.set(what, entry);
  };
  for (const event of log.events) {
    const type = eventName.get(event.type) ?? `event ${event.type}`;
    const params = event.params ?? {};
    const id = event.source?.id;
    if (!sources.has(id)) sources.set(id, { outside: [], loopback: false, datagrams: 0, refused: [] });
    const source = sources.get(id);
    if (DATAGRAM_EVENTS.includes(type)) source.datagrams += 1;
    if (params.net_error === ERR_ACCESS_DENIED) source.refused.push(type);
    if (typeof params.source_dependency?.id === "number" && !madeBy.has(id)) madeBy.set(id, params.source_dependency.id);
    if (type === "HOST_RESOLVER_MANAGER_IPV6_REACHABILITY_CHECK" && params.cached === false) {
      uncachedIpv6ChecksBySource.set(id, (uncachedIpv6ChecksBySource.get(id) ?? 0) + 1);
    }
    for (const key of ADDRESS_KEYS) {
      const address = params[key];
      if (typeof address !== "string") continue;
      if (LOOPBACK.test(address)) source.loopback = true;
      else source.outside.push({ type, address });
    }
    if (type === "PROXY_CONFIG_CHANGED" && params.new_config?.from_system === true) systemProxy = true;
    if (type === "HOST_RESOLVER_MANAGER_REQUEST" && typeof params.host === "string") {
      const name = nameSentToResolver(params.host);
      if (name !== null) note(`lookup ${name}`, `name ${name}`);
    }
  }
  const checker = (id) => {
    let maker = madeBy.get(id);
    for (let hop = 0; hop < MOST_HOPS_FROM_A_PROBE_SOCKET_TO_ITS_CHECK && maker !== undefined; hop += 1) {
      if (uncachedIpv6ChecksBySource.has(maker)) return maker;
      maker = madeBy.get(maker);
    }
    return undefined;
  };
  const accounted = new Map();
  const stillOwedAProbe = (check, type) =>
    check !== undefined && (accounted.get(`${check} ${type}`) ?? 0) < uncachedIpv6ChecksBySource.get(check);
  const probes = Object.fromEntries(PROBE_EVENTS.map((type) => [type, 0]));
  for (const [id, source] of sources) {
    if (source.outside.length === 0 && source.loopback) {
      for (const type of source.refused) {
        note(`${type} that macOS refused on a socket that named a loopback address`, "a loopback address");
      }
      continue;
    }
    if (source.outside.length === 0) {
      for (let sent = 0; sent < source.datagrams; sent += 1) {
        note("a datagram on a socket whose address the log does not give", "unnamed");
      }
      continue;
    }
    const probeShaped =
      source.datagrams === 0 &&
      source.outside.every(({ type, address }) => address === IPV6_REACHABILITY_CHECK_ADDRESS && PROBE_EVENTS.includes(type));
    const check = probeShaped ? checker(id) : undefined;
    for (const { type, address } of source.outside) {
      if (stillOwedAProbe(check, type)) {
        const key = `${check} ${type}`;
        accounted.set(key, (accounted.get(key) ?? 0) + 1);
        probes[type] += 1;
      } else note(`${type} ${address}`, address);
    }
  }
  return {
    systemProxy,
    events: log.events.length,
    probes: socketsThatEachShowAsOneSourceOrTwo(probes),
    attempts: [...attemptsByAddress(kinds).values()].reduce((sum, count) => sum + count, 0),
    kinds: [...kinds].map(([what, { count }]) => `${count} x ${what}`).sort(),
  };
}

const finishedByChromium = (log) => "polledData" in log;

function cannotCount(why) {
  console.log(`cannot count: ${why}`);
  process.exit(2);
}

if (import.meta.main) {
  const file = process.argv[2];
  if (!file) {
    console.error("usage: node gate/lib/netlog.mjs FILE");
    process.exit(64);
  }
  let text;
  try {
    text = readFileSync(file, "utf8");
  } catch {
    cannotCount(`the browser left no net log (${file})`);
  }
  let log;
  try {
    log = JSON.parse(text);
  } catch {
    cannotCount(`the browser's net log is not complete (${file})`);
  }
  if (!Array.isArray(log?.events) || !log.constants?.logEventTypes || !finishedByChromium(log)) {
    cannotCount(`the browser's net log is not complete (${file})`);
  }
  if (log.events.length === 0) cannotCount(`the browser's net log holds no event (${file})`);
  const counted = countOutside(log);
  if (counted.systemProxy) {
    cannotCount("the browser took its proxy settings from macOS, so a proxy could carry a request out unseen");
  }
  console.log(`events ${counted.events}`);
  console.log(`probes ${counted.probes}`);
  console.log(`attempts ${counted.attempts}`);
  for (const kind of counted.kinds) console.log(`attempt ${kind}`);
}
