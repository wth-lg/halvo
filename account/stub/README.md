# account/stub

A stand-in for the account backend's app-facing routes. Tests of the helper `halvo-account` start it on a loopback port in place of the backend. It reads only three vendored files of `account/schema/`: `schema.v1.json`, `routes.v1.json` and `samples.v1.json`. It reads no source of the backend.

It is a Node package of its own, outside the Rust workspace of `account/`. It needs Node 26 or later and has no dependency, so no prerequisites step installs anything for it. Its self-test also runs an OpenSSL 3 binary, which it looks for in Homebrew's places and on the PATH, or takes from the variable `STUB_OPENSSL`.

## Commands

Start each command at the repository root.

| Command | What it does |
|---|---|
| `node account/stub/src/stub.mjs --record DIR` | Serves the app-facing routes over HTTP and records every request in `DIR`. |
| `node account/stub/src/stub.mjs --record DIR --tls-ca FILE` | Serves the app-facing routes over HTTPS, as the next section says. |
| `node --test --test-concurrency=1 --test-timeout=60000 --test-force-exit account/stub/test/*.test.mjs` | Runs the stub's self-test, one file at a time, ending a test after 60 s and the run after the last test. |
| `sh scripts/ci/run-ci-d.sh macos 20-r7-stub` | Runs the check that the CI jobs run: the self-test and the package rule. |

`DIR` must be an empty directory that exists. The stub listens on two ports of `127.0.0.1` that the system picks: one for the app-facing routes and one for the test calls. When it is ready, it prints one line on standard output and nothing more:

```json
{"app":"http://127.0.0.1:50311","control":"http://127.0.0.1:50312"}
```

It runs until the test call `POST /stop`, SIGINT or SIGTERM, ends every open connection, and exits 0. It exits 64 and serves nothing when an argument is unknown or given twice, when `--record` is missing, when `DIR` is missing, not a directory or not empty, and when `FILE` exists or its directory does not. When it cannot write the record, it answers nothing and exits with an error.

## TLS with a test authority

With `--tls-ca FILE`, the stub makes a test certificate authority and a server certificate when it starts. Both are made in memory:

- The keys are ECDSA P-256, and the certificates are signed with SHA-256.
- The server certificate names the address `127.0.0.1` and no host name.
- Both certificates are valid from one hour before the start to two days after it.
- Basic constraints and key usage are critical extensions of both, so that a strict verifier such as OpenSSL's `-x509_strict` takes the chain.

The stub writes the authority's certificate to `FILE`, and the app-facing port serves HTTPS. The control port stays HTTP. No private key is written to a file, and nothing is added to a trust store. Give `FILE` to the client under test, for example as a root certificate of its TLS client. A client that is not given `FILE` refuses the stub's certificate. So does a client that checks a host name such as `localhost`.

## How a request is read

The app-facing port reads HTTP/1.1 itself, so that every request reaches the checks of the next section, whatever its `expect`, `upgrade` or `connection` header, its method's case, and whether it names a host. `CONNECT` is a method like any other. A connection serves its requests in order, and the stub closes it after an answer to a request that asks for `connection: close` and after every answer to HTTP/1.0. An answer to `HEAD` carries its head alone. The stub sends no `100 Continue`.

A connection that sends nothing for 5,000 ms (`IDLE_MS` of `src/stub.mjs`) is ended. When a byte of a request had arrived, it gets the 400 of the next paragraph with the cause `idle`; a request whose lines end in a bare LF is one such case, because the stub reads only CRLF as a line end. When no byte of a request had arrived, as on a keep-alive connection after its last answer, the stub closes it without an answer and records the line `idle-close`. The limit counts from the last byte received. It is far above any gap between the bytes of one request on loopback, and below the 10 s that a test of the stub waits for an answer.

Bytes that are no request get status 400 with `content-type: application/json` and the body `{"result":"rejected"}`, as contract 1.4 answers a body that does not parse, and the stub closes the connection and reads no more of it. The causes are:

| Cause | The bytes |
|---|---|
| `request-line` | A request line that is not a method token, one space, a target with no space, one space and `HTTP/1.0` or `HTTP/1.1`. |
| `field-line` | A header or trailer line that is not a name token, a colon and a value with no CR, LF or NUL, a folded line among them. |
| `framing` | Both `transfer-encoding` and `content-length`. |
| `transfer-encoding` | A `transfer-encoding` other than one `chunked`. |
| `content-length` | A `content-length` that is not 1 to 15 digits, or two that differ. |
| `chunk` | A chunk size that is not 1 to 13 hexadecimal digits, a chunk not followed by CRLF, or a chunk-size line above 4,096 bytes. |
| `head-too-large` | A request head, or the trailer section of a chunked body, above 65,536 bytes. |
| `incomplete` | A peer that ends its side inside a request. |
| `idle` | A peer that sends nothing for the idle limit inside a request. |

## How a request is judged

The checks run in the order of contract 1.4 of the billing run, and the first check that fails answers:

1. The path is compared with the routes of `routes.v1.json` after dot segments are resolved. An unknown path gets 404, and a known path with another method gets 405.
2. A request target that holds `?` gets 400.
3. On an app route, a `halvo-account` or `authorization` header that is missing or does not match its pattern in `routes.v1.json` gets 400. Two headers of one name are joined with `, ` and fail the pattern.
4. A route that takes no body gets 400 for a body of one byte or more. The webhook route gets 413 for a body above its `limit`.
5. A route with a request schema gets 415 for a `content-type` that does not match its pattern, then 413 for a body above its `limit`. It gets 400 for a body that is no JSON, that is not byte-equal to `JSON.stringify` of its parsed value, or that fails the schema.

Every rejection answers with that status, `content-type: application/json` and the body `{"result":"rejected"}`. A rejection changes no state. An accepted request on an app route gets status 200 with a JSON answer that the route's answer schema allows, with exactly the keys that `results` lists for its `result`. Trailer fields are recorded and take no part in these checks.

## States and test calls

The stub keeps one state for each account id that the `halvo-account` header names. It stores the SHA-256 of the credential, and it never stores the credential.

| State | `POST /v1/checkout` | `GET /v1/subscription` | `GET /v1/cap`, `PUT /v1/cap` | `POST /v1/manage` | `DELETE /v1/account` |
|---|---|---|---|---|---|
| nothing | `session`, and the id becomes a pending checkout with the body's cap | `no-such-account` | `no-such-account` | `no-such-account` | `no-such-account` |
| pending checkout | `session`, and the pending cap becomes the body's cap | `pending` | `pending` | `pending` | `pending` |
| account | `account-exists` | `active`, `payment-failed` or `cancelled` | `cap`; `PUT` stores the body's cap first | `page` | `deleted`, and the id holds nothing again |

When the id holds a pending checkout or an account and the credential's hash differs, every route answers `credential-mismatch` and nothing changes.

The test calls are `POST` requests to the control port. Each one answers `{"changed":N}`, where N is the number of accounts that it changed, and any other request gets 404 with an empty body.

| Test call | What it does |
|---|---|
| `POST /complete` | Every pending checkout becomes an account in the state `active`, with the pending cap, and the end of its first cap period is set from the time of this call. |
| `POST /expire` | Every pending checkout is dropped, so its id answers `no-such-account`. |
| `POST /cancel` | Every account in the state `active` or `payment-failed` becomes `cancelled`. |
| `POST /fail-payment` | Every account in the state `active` becomes `payment-failed`. |
| `POST /arm-not-finished` | The next `DELETE /v1/account` that would delete an account answers `not-finished` and deletes nothing. N is 1, or 0 when it was armed already. |
| `POST /stop` | Answers `{"stopped":true}` and stops the stub. |

## Fixed values

The schema leaves these values open, and the stub fixes them:

- The `url` of `session` and of `page` is the samples' value, `http://127.0.0.1:1/x`.
- The `spent` and `held` of a `cap` answer are the samples' 0.
- The `period_end` of a `cap` answer is the end of the account's first cap period as contract 4 counts it from the time of `POST /complete`: 604,800,000 ms later for the period `week`, and for `month` the same day and time of day one calendar month later in UTC, or that month's last day. The stub does not roll the period, and `PUT /v1/cap` does not move it.
- The stub never answers `unavailable` or `no-cap`. It has no provider that can fail, and every account gets its cap from its checkout.
- The webhook route answers every body at or under its `limit` with 400 and the rejected body, because the stub checks no provider signature.
- `GET /v1/done` answers 200 with `content-type: text/html; charset=utf-8` and a fixed page that holds no script, no link and no value from the request.
- An answer of the app-facing port carries `content-length`, `content-type`, `connection: close` when the stub closes the connection after it, and no other header.
- The stub keeps the first 1,048,576 bytes (1 MiB) of a body, and reads the rest of the body without keeping it.

## The record

The record directory holds these files:

| File | What it holds |
|---|---|
| `connection-NNNNNN.raw` | Every byte that connection NNNNNN received, in order. Over TLS it holds the plain text. |
| `request-NNNNNN.body` | The bytes that the stub kept of the body of request NNNNNN. |
| `requests.jsonl` | One line for each request, written before its answer is sent. |

A line of `requests.jsonl` has these keys:

- `seq` and `connection`, the numbers of the line and of its connection.
- `method` and `target`, as received.
- `headers` and `trailers`, each a list of `[name, value]` pairs in the order received. `trailers` holds the trailer fields of a chunked body and is empty for any other body.
- `body`, the name of the body file, and `bytes`, the number of bytes in it. Above 1 MiB, `bytes` is the kept count, not the count received; the raw file holds every byte.
- `status` and `result`, the answer's status and `result`, with `result` null for the done page.

The line writes each byte of a target, a name or a value as one character, its latin1 reading, so a byte above 0x7F stands as one character and not as UTF-8. A search for a value sent as UTF-8 reads the raw files.

Bytes that are no request get the line `{"seq", "connection", "error"}`, where `error` is the cause. A connection that the idle limit closes with no request begun gets the same line with `error` set to `idle-close`. A TLS handshake that the server sees fail, such as one that the client ends with an alert, gets a line with `seq` and `error` only, and `error` is Node's code. A client that refuses the certificate and closes without an alert leaves no line, and neither does a connection that the peer resets.

The stub writes no value of a request to standard output or standard error.
