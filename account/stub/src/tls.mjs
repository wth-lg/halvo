import { createHash, generateKeyPairSync, randomBytes, sign } from "node:crypto";
import { writeFileSync } from "node:fs";

/** Thrown when the file for the authority's certificate cannot be made: it exists, or its directory does not. */
export class TlsError extends Error {
  constructor(message) {
    super(message);
    this.name = "TlsError";
  }
}

const HOUR = 3600000;
const DAY = 24 * HOUR;

function length(n) {
  if (n < 0x80) return Buffer.from([n]);
  const bytes = [];
  for (let rest = n; rest > 0; rest = Math.floor(rest / 256)) bytes.unshift(rest % 256);
  return Buffer.from([0x80 | bytes.length, ...bytes]);
}

const der = (tag, ...parts) => {
  const body = Buffer.concat(parts);
  return Buffer.concat([Buffer.from([tag]), length(body.length), body]);
};
const seq = (...parts) => der(0x30, ...parts);
const explicit = (n, part) => der(0xa0 + n, part);
const octets = (bytes) => der(0x04, bytes);
const bits = (bytes, unused = 0) => der(0x03, Buffer.from([unused]), bytes);
const TRUE = Buffer.from([0x01, 0x01, 0xff]);

function oid(text) {
  const [first, second, ...rest] = text.split(".").map(Number);
  const out = [40 * first + second];
  for (const arc of rest) {
    const groups = [arc & 0x7f];
    for (let value = arc >>> 7; value > 0; value >>>= 7) groups.unshift(0x80 | (value & 0x7f));
    out.push(...groups);
  }
  return der(0x06, Buffer.from(out));
}

const utcTime = (ms) => {
  const text = new Date(ms).toISOString();
  return der(0x17, Buffer.from(`${text.slice(2, 4)}${text.slice(5, 7)}${text.slice(8, 10)}${text.slice(11, 13)}${text.slice(14, 16)}${text.slice(17, 19)}Z`, "ascii"));
};

const name = (common) => seq(der(0x31, seq(oid("2.5.4.3"), der(0x0c, Buffer.from(common, "utf8")))));
const extension = (id, critical, value) => seq(oid(id), ...(critical ? [TRUE] : []), octets(value));
const ECDSA_SHA256 = seq(oid("1.2.840.10045.4.3.2"));

function serial() {
  const bytes = randomBytes(16);
  bytes[0] = (bytes[0] & 0x3f) | 0x40;
  return der(0x02, bytes);
}

function keyId(spki) {
  return createHash("sha1").update(spki.subarray(spki.length - 65)).digest();
}

function certificate({ subject, issuer, spki, signer, from, to, extensions }) {
  const tbs = seq(explicit(0, der(0x02, Buffer.from([2]))), serial(), ECDSA_SHA256, issuer, seq(utcTime(from), utcTime(to)), subject, spki, explicit(3, seq(...extensions)));
  return seq(tbs, ECDSA_SHA256, bits(sign("sha256", tbs, signer)));
}

const pem = (label, bytes) => `-----BEGIN ${label}-----\n${bytes.toString("base64").replace(/.{64}/g, "$&\n").replace(/\n?$/, "\n")}-----END ${label}-----\n`;

/**
 * Makes, in memory, a test certificate authority and a server certificate that it signs for the address 127.0.0.1
 * and no name: ECDSA P-256 keys, signed with SHA-256, valid from one hour before `now` to two days after it.
 * Returns `{caPem, certPem, keyPem}`: the authority's certificate, the server certificate and the server's private key.
 * The authority's private key is never returned or written.
 */
export function makeTestAuthority(now) {
  const from = now - HOUR;
  const to = now + 2 * DAY;
  const authority = generateKeyPairSync("ec", { namedCurve: "P-256" });
  const server = generateKeyPairSync("ec", { namedCurve: "P-256" });
  const authoritySpki = authority.publicKey.export({ type: "spki", format: "der" });
  const serverSpki = server.publicKey.export({ type: "spki", format: "der" });
  const authorityName = name("halvo account stub test authority");
  const caDer = certificate({
    subject: authorityName,
    issuer: authorityName,
    spki: authoritySpki,
    signer: authority.privateKey,
    from,
    to,
    extensions: [
      extension("2.5.29.19", true, seq(TRUE)),
      extension("2.5.29.15", true, bits(Buffer.from([0x06]), 1)),
      extension("2.5.29.14", false, octets(keyId(authoritySpki))),
    ],
  });
  const certDer = certificate({
    subject: name("halvo account stub"),
    issuer: authorityName,
    spki: serverSpki,
    signer: authority.privateKey,
    from,
    to,
    extensions: [
      extension("2.5.29.19", true, seq()),
      extension("2.5.29.15", true, bits(Buffer.from([0x80]), 7)),
      extension("2.5.29.37", false, seq(oid("1.3.6.1.5.5.7.3.1"))),
      extension("2.5.29.17", false, seq(der(0x87, Buffer.from([127, 0, 0, 1])))),
      extension("2.5.29.14", false, octets(keyId(serverSpki))),
      extension("2.5.29.35", false, seq(der(0x80, keyId(authoritySpki)))),
    ],
  });
  return {
    caPem: pem("CERTIFICATE", caDer),
    certPem: pem("CERTIFICATE", certDer),
    keyPem: server.privateKey.export({ type: "pkcs8", format: "pem" }),
  };
}

/** Writes the authority's certificate to `file`, which must not exist yet; throws TlsError otherwise. */
export function writeAuthority(file, caPem) {
  try {
    writeFileSync(file, caPem, { flag: "wx" });
  } catch {
    throw new TlsError("the file for the authority's certificate exists or its directory does not");
  }
}
