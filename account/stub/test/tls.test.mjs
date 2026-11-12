import { test } from "node:test";
import assert from "node:assert/strict";
import { X509Certificate } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { connect } from "node:tls";
import { connect as netConnect } from "node:net";
import { join } from "node:path";
import { makeTestAuthority, TlsError } from "../src/tls.mjs";
import { exchange, request, parse, appHeaders, portOf, spawnStub, exited, fresh, startStub } from "./support.mjs";

const SERVER_AUTH = "1.3.6.1.5.5.7.3.1";
const DAY = 86400000;
const HOUR = 3600000;

test("the test authority signs a server certificate for 127.0.0.1 alone, valid for a short time around now", () => {
  const now = Date.now();
  const made = makeTestAuthority(now);
  const ca = new X509Certificate(made.caPem);
  const leaf = new X509Certificate(made.certPem);
  assert.equal(ca.ca, true);
  assert.equal(leaf.ca, false);
  assert.equal(ca.verify(ca.publicKey), true);
  assert.equal(ca.checkIssued(ca), true);
  assert.equal(leaf.checkIssued(ca), true);
  assert.equal(leaf.verify(ca.publicKey), true);
  assert.equal(leaf.verify(leaf.publicKey), false);
  assert.equal(leaf.checkIP("127.0.0.1"), "127.0.0.1");
  assert.equal(leaf.checkIP("127.0.0.2"), undefined);
  assert.equal(leaf.checkHost("localhost"), undefined);
  assert.equal(leaf.subjectAltName, "IP Address:127.0.0.1");
  assert.deepEqual(leaf.keyUsage, [SERVER_AUTH]);
  assert.equal(leaf.publicKey.asymmetricKeyDetails.namedCurve, "prime256v1");
  for (const cert of [ca, leaf]) {
    assert.ok(cert.validFromDate.getTime() <= now && now < cert.validToDate.getTime(), cert.subject);
    assert.equal(cert.validFromDate.getTime(), Math.floor((now - HOUR) / 1000) * 1000, cert.subject);
    assert.equal(cert.validToDate.getTime(), Math.floor((now + 2 * DAY) / 1000) * 1000, cert.subject);
  }
  assert.equal(leaf.issuer, ca.subject);
  const again = makeTestAuthority(now);
  assert.notEqual(new X509Certificate(again.caPem).serialNumber, ca.serialNumber);
  assert.notEqual(new X509Certificate(again.certPem).fingerprint256, leaf.fingerprint256);
  assert.match(made.keyPem, /^-----BEGIN PRIVATE KEY-----\n/);
  assert.doesNotMatch(made.caPem, /PRIVATE/);
});

function tlsExchange(port, bytes, options) {
  return exchange(port, bytes, (where) => connect({ ...where, ...options }));
}

test("over TLS the stub serves and records in plain text for a client that is given the authority's file", async () => {
  const keys = fresh();
  const record = fresh();
  const caFile = join(keys, "ca.pem");
  const stub = await startStub({ record, tlsCa: caFile });
  try {
    assert.equal(new URL(stub.app).protocol, "https:");
    assert.equal(new URL(stub.control).protocol, "http:");
    assert.deepEqual(readdirSync(keys), ["ca.pem"]);
    const ca = readFileSync(caFile);
    assert.doesNotMatch(ca.toString("latin1"), /PRIVATE/);
    const bytes = request("GET", "/v1/subscription", appHeaders());
    const answer = parse(await tlsExchange(portOf(stub.app), bytes, { ca }));
    assert.equal(answer.status, 200);
    assert.equal(answer.body, '{"result":"no-such-account"}');
    const lines = readFileSync(join(record, "requests.jsonl"), "utf8").trim().split("\n").map((line) => JSON.parse(line));
    assert.deepEqual(lines.map((line) => [line.connection, line.target, line.status]), [[1, "/v1/subscription", 200]]);
    assert.deepEqual(readFileSync(join(record, "connection-000001.raw")), bytes);
    for (const name of readdirSync(record)) assert.doesNotMatch(readFileSync(join(record, name)).toString("latin1"), /PRIVATE KEY/, name);
  } finally {
    await stub.close();
  }
});

test("a client that is not given the authority, or that checks another name, refuses the stub's certificate", async () => {
  const record = fresh();
  const stub = await startStub({ record, tlsCa: join(fresh(), "ca.pem") });
  const other = makeTestAuthority(Date.now());
  try {
    const port = portOf(stub.app);
    const bytes = request("GET", "/v1/subscription", appHeaders());
    await assert.rejects(tlsExchange(port, bytes, {}), (error) => /CERT|SIGNATURE|ISSUER/.test(error.code));
    await assert.rejects(tlsExchange(port, bytes, { ca: other.caPem }), (error) => /CERT|SIGNATURE|ISSUER/.test(error.code));
    assert.equal(readFileSync(join(record, "requests.jsonl"), "utf8"), "");
  } finally {
    await stub.close();
  }
});

test("a connection that fails the TLS handshake is recorded with the error's code and no request", async () => {
  const record = fresh();
  const stub = await startStub({ record, tlsCa: join(fresh(), "ca.pem") });
  try {
    await exchange(portOf(stub.app), request("GET", "/v1/subscription", appHeaders()));
    const lines = readFileSync(join(record, "requests.jsonl"), "utf8").trim().split("\n").map((line) => JSON.parse(line));
    assert.equal(lines.length, 1);
    assert.deepEqual(Object.keys(lines[0]).sort(), ["error", "seq"]);
    assert.equal(lines[0].seq, 1);
    assert.match(lines[0].error, /^ERR_SSL_/);
  } finally {
    await stub.close();
  }
});

test("a connection that the peer resets inside the TLS handshake leaves no line", async () => {
  const record = fresh();
  const stub = await startStub({ record, tlsCa: join(fresh(), "ca.pem") });
  const socket = netConnect({ host: "127.0.0.1", port: portOf(stub.app) });
  socket.on("error", () => {});
  await new Promise((resolve) => socket.on("connect", resolve));
  socket.write(Buffer.from([0x16, 0x03, 0x01, 0x00, 0x40, 0x01, 0x00, 0x00, 0x3c]));
  await new Promise((resolve) => setTimeout(resolve, 200));
  socket.resetAndDestroy();
  await new Promise((resolve) => setTimeout(resolve, 300));
  assert.equal(readFileSync(join(record, "requests.jsonl"), "utf8"), "");
});

test("a client that checks the name localhost refuses the certificate, which names only the address", async () => {
  const keys = fresh();
  const stub = await startStub({ record: fresh(), tlsCa: join(keys, "ca.pem") });
  try {
    const ca = readFileSync(join(keys, "ca.pem"));
    const bytes = request("GET", "/v1/subscription", appHeaders());
    await assert.rejects(tlsExchange(portOf(stub.app), bytes, { ca, servername: "localhost" }), (error) => error.code === "ERR_TLS_CERT_ALTNAME_INVALID");
  } finally {
    await stub.close();
  }
});

test("the authority's file is never written over, and a refused start writes no file", async () => {
  const keys = fresh();
  const caFile = join(keys, "ca.pem");
  writeFileSync(caFile, "keep");
  const record = fresh();
  await assert.rejects(startStub({ record, tlsCa: caFile }), TlsError);
  assert.equal(readFileSync(caFile, "utf8"), "keep");
  assert.deepEqual(readdirSync(record), []);
  await assert.rejects(startStub({ record, tlsCa: join(keys, "missing", "ca.pem") }), TlsError);
  assert.deepEqual(readdirSync(record), []);
  const full = fresh();
  writeFileSync(join(full, "x"), "");
  const second = join(fresh(), "ca.pem");
  await assert.rejects(startStub({ record: full, tlsCa: second }));
  assert.deepEqual(readdirSync(join(second, "..")), []);
});

test("the command line serves over TLS with --tls-ca and refuses a file that exists", async () => {
  const keys = fresh();
  const caFile = join(keys, "ca.pem");
  const { child, ready } = await spawnStub(["--record", fresh(), "--tls-ca", caFile]);
  assert.equal(new URL(ready.app).protocol, "https:");
  const answer = parse(await tlsExchange(portOf(ready.app), request("GET", "/v1/cap", appHeaders()), { ca: readFileSync(caFile) }));
  assert.equal(answer.body, '{"result":"no-such-account"}');
  await exchange(portOf(ready.control), request("POST", "/stop"));
  assert.equal(await exited(child), 0);
  const refused = fresh();
  const again = await spawnStub(["--record", refused, "--tls-ca", caFile]);
  assert.equal(again.ready, null);
  assert.equal(await exited(again.child), 64);
  assert.deepEqual(readdirSync(refused), []);
  const twice = await spawnStub(["--record", fresh(), "--tls-ca", join(fresh(), "a.pem"), "--tls-ca", join(fresh(), "b.pem")]);
  assert.equal(await exited(twice.child), 64);
});

function tlv(der, at) {
  let length = der[at + 1];
  let start = at + 2;
  if (length & 0x80) {
    const count = length & 0x7f;
    length = 0;
    for (let index = 0; index < count; index += 1) length = length * 256 + der[start + index];
    start += count;
  }
  return { tag: der[at], start, end: start + length };
}

function children(der, node) {
  const out = [];
  for (let at = node.start; at < node.end; at = out[out.length - 1].end) out.push(tlv(der, at));
  return out;
}

function extensions(pem) {
  const der = new X509Certificate(pem).raw;
  const tbs = children(der, tlv(der, 0))[0];
  const block = children(der, tbs).find((node) => node.tag === 0xa3);
  return children(der, children(der, block)[0]).map((node) => {
    const [id, second] = children(der, node);
    const critical = second.tag === 0x01 && der[second.start] === 0xff;
    return [der.subarray(id.start, id.end).toString("hex"), critical];
  });
}

test("both certificates carry basic constraints and key usage as critical extensions, and no other critical one", () => {
  const made = makeTestAuthority(Date.now());
  assert.deepEqual(extensions(made.caPem), [["551d13", true], ["551d0f", true], ["551d0e", false]]);
  assert.deepEqual(extensions(made.certPem), [["551d13", true], ["551d0f", true], ["551d25", false], ["551d11", false], ["551d0e", false], ["551d23", false]]);
});

function openssl3() {
  const candidates = [process.env.STUB_OPENSSL, "/opt/homebrew/opt/openssl@3/bin/openssl", "/opt/homebrew/bin/openssl", "/usr/local/opt/openssl@3/bin/openssl", "openssl"];
  for (const candidate of candidates.filter(Boolean)) {
    const run = spawnSync(candidate, ["version"], { encoding: "utf8" });
    if (run.status === 0 && /^OpenSSL 3\./.test(run.stdout)) return candidate;
  }
  return null;
}

test("OpenSSL 3 under -x509_strict takes the server certificate for 127.0.0.1 with the authority as the only anchor", () => {
  const openssl = openssl3();
  assert.notEqual(openssl, null, "no OpenSSL 3 binary found; set STUB_OPENSSL to one");
  const dir = fresh();
  const made = makeTestAuthority(Date.now());
  const other = makeTestAuthority(Date.now());
  writeFileSync(join(dir, "ca.pem"), made.caPem);
  writeFileSync(join(dir, "other.pem"), other.caPem);
  writeFileSync(join(dir, "cert.pem"), made.certPem);
  const verify = (ca, ip) => spawnSync(openssl, ["verify", "-x509_strict", "-purpose", "sslserver", "-verify_ip", ip, "-CAfile", join(dir, ca), join(dir, "cert.pem")], { encoding: "utf8" });
  const good = verify("ca.pem", "127.0.0.1");
  assert.equal(good.status, 0, good.stdout + good.stderr);
  assert.match(good.stdout, /: OK/);
  assert.notEqual(verify("other.pem", "127.0.0.1").status, 0);
  assert.notEqual(verify("ca.pem", "127.0.0.2").status, 0);
});
