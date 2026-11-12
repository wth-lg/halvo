const file = process.env.HALVO_OUTBOUND_AUDIT;
if (file) {
  const fs = require("node:fs");
  const net = require("node:net");
  const dns = require("node:dns");
  const dgram = require("node:dgram");

  const isLoopback = (host) => {
    if (host === undefined || host === null || host === "") return true;
    const name = String(host).toLowerCase().replace(/^\[|\]$/g, "");
    return (
      name === "localhost" ||
      name.endsWith(".localhost") ||
      name === "::1" ||
      /^127\.\d+\.\d+\.\d+$/.test(name) ||
      /^::ffff:127\.\d+\.\d+\.\d+$/.test(name)
    );
  };
  const appendRecordBeforeAttempt = (kind, host) => {
    try {
      fs.appendFileSync(file, `${process.pid} ${kind} ${host}\n`);
    } catch {}
  };
  const recordUnlessLoopback = (kind, host) => {
    if (!isLoopback(host)) appendRecordBeforeAttempt(kind, host);
  };
  const replace = (owner, method, wrapper) => {
    Object.defineProperties(wrapper, Object.getOwnPropertyDescriptors(owner[method]));
    owner[method] = wrapper;
  };

  const namesLocalSocket = (options) => options.path !== undefined && options.path !== null;

  const connect = net.Socket.prototype.connect;
  replace(net.Socket.prototype, "connect", function (...args) {
    const first = Array.isArray(args[0]) ? args[0][0] : args[0];
    if (first !== null && typeof first === "object") {
      if (!namesLocalSocket(first)) recordUnlessLoopback("connect", first.host);
    } else if (typeof first === "number" || /^\d+$/.test(String(first))) {
      recordUnlessLoopback("connect", typeof args[1] === "string" ? args[1] : undefined);
    }
    return connect.apply(this, args);
  });

  const recordEveryQuery = (owner, method, kind) => {
    const original = owner[method];
    if (typeof original !== "function") return;
    const answersAnAddressLocally = method === "lookup";
    replace(owner, method, function (host, ...rest) {
      if (typeof host === "string" && !(answersAnAddressLocally && net.isIP(host) !== 0)) appendRecordBeforeAttempt(kind, host);
      return original.call(this, host, ...rest);
    });
  };
  const queries = (owner) => Object.getOwnPropertyNames(owner).filter((name) => /^(resolve|reverse)/.test(name));
  for (const owner of [dns, dns.promises]) {
    recordEveryQuery(owner, "lookup", "lookup");
    for (const method of queries(owner)) recordEveryQuery(owner, method, "resolve");
  }
  for (const owner of [dns.Resolver.prototype, dns.promises.Resolver.prototype]) {
    for (const method of queries(owner)) recordEveryQuery(owner, method, "resolve");
  }

  const send = dgram.Socket.prototype.send;
  replace(dgram.Socket.prototype, "send", function (message, ...rest) {
    const address = rest.find((value) => typeof value === "string");
    if (address !== undefined) recordUnlessLoopback("datagram", address);
    return send.call(this, message, ...rest);
  });
  const datagramConnect = dgram.Socket.prototype.connect;
  replace(dgram.Socket.prototype, "connect", function (port, address, ...rest) {
    if (typeof address === "string") recordUnlessLoopback("datagram", address);
    return datagramConnect.call(this, port, address, ...rest);
  });

  require("node:module").syncBuiltinESMExports();
}
