# The gateway request

This file states what a request to the model gateway may hold and how a device reads the answer. It is written for the reader of the billing run, which owns the gateway, its answers and the adapter to a model host. The routing run owns this contract (its item T11). The crate `halvo-route-wire` holds it as code, and the files of this directory hold it as data that both sides test against. Nothing in the app sends a request yet.

## The envelope

- The path is `POST /v1/chat`.
- The account travels in the two headers of the billing run's contract 1.2: `halvo-account` and `authorization`.
- Every request carries the header `x-request-id`: a version 4 UUID in lower case that the sender mints from 16 random bytes. A second send after no answer carries the same id and the same body.
- The body travels as `content-type: application/json`.

## The body

The body is one JSON object with exactly these keys, written in this order with no space:

```
{"tier":"low_cost_hosted"|"frontier","effort":"low"|"medium"|"high","max_output_tokens":<count>,"messages":[{"role":"system"|"user"|"assistant","content":"<text>"}...]}
```

- `tier` is the tier that the router picked. It is a ceiling: the gateway never serves above it and prices the hold at its rates. `on_device` is never a value.
- `effort` is the effort level, which the gateway maps to the host's setting. The key is `effort` and not `reasoning_level`.
- `max_output_tokens` is the output limit of the one choice that a request asks for. The body has no key for a number of choices.
- A message is text. The body holds no image, no file reference, no URL part, no stored conversation and no tool that the provider runs. It names no workflow, agent, run, category or site.

A reader accepts the keys in any order and JSON white space, which is space, tab, LF and CR, before the object, between tokens and after the object, decodes escapes in names and values, and refuses a repeated key. `max_output_tokens` is digits alone with no leading zero but `0` itself: a sign, a fraction, an exponent, a leading zero or a value above 4,294,967,295 is refused. `content` may be empty. A writer writes the keys in the order above with no space, and writes a string as `serde_json` writes one: `\"`, `\\`, the short escapes `\b`, `\f`, `\n`, `\r` and `\t`, `\u00xx` with lower-case digits for the other characters under 0x20, and every other character as UTF-8.

A reader reads the bytes as received. The size of a body is the count of those bytes before any decoding. A leading EF BB BF, and any byte sequence that is no UTF-8, an overlong form and an encoded surrogate among them, is `malformed`, and so is an escape of a lone surrogate anywhere in the body, also in the value of an unknown key. A gateway on a Worker takes the raw bytes of the request with `arrayBuffer()` and decodes them with a decoder that fails on a wrong byte and keeps a leading EF BB BF, `new TextDecoder("utf-8", {fatal: true, ignoreBOM: true})`; `request.json()` and `request.text()` do neither.

A refused body gets one of these errors. Where two fit, the first of this order is the answer:

| Error | When |
|---|---|
| `size` | The body is more than 131072 bytes as received. |
| `malformed` | The bytes are not one JSON object of UTF-8 text, a value has the wrong JSON type, a count is not written as above, or a string holds an escape of a lone surrogate. |
| `key` | A key is unknown, missing or repeated, in the body or in a message. |
| `tier` | `tier` is not one of its two words. |
| `effort` | `effort` is not one of its three words. |
| `role` | A `role` is not one of its three words. |
| `limit` | The body holds 0 or more than 64 messages, or `max_output_tokens` is 0 or more than 2048. |

`size` comes first: a body of more than 131072 bytes as received is `size` whatever else it holds, so a gateway may refuse such a body without reading it. The vectors `size_before_limit` and `size_before_malformed` hold that case.

In a body of at most 131072 bytes every fault that fits anywhere counts, and the error is the first of the order among them: a reader does not stop at the first fault it meets, so a repeated key whose second value has the wrong JSON type is `malformed`, and a missing key beside a wrong `effort` is `key`.

The rule of digits binds `max_output_tokens` and nothing else: the value of an unknown key is any JSON value, at any depth that the size allows, and a number inside it is any JSON number, so `"x":1.5` is `key`.

## The limits

`tier-limits.tsv` holds the limits of each tier. Both tiers have the same values today, and all five are provisional: no hosted step has run, and the first recorded hosted steps replace them.

| Column | Value | Meaning |
|---|---|---|
| `max_body_bytes` | 131072 | The largest body in bytes. |
| `max_output_tokens` | 2048 | The largest output limit. |
| `hold_life_ms` | 120000 | The life of the hold. A device that has no answer waits the hold's life and an allowance that the branch sets before its second send. |
| `upstream_deadline_ms` | 90000 | The least time that must be left on a hold for a dispatch, in the billing run's meaning. The dispatch margin stays the billing run's. |
| `status` | `provisional` | No value is measured. |

The two times must fit under the limit that the billing run reports for a Worker whose client has gone.

## The two answers

The device reads two answers, each one complete body that the gateway sends once the upstream call has ended. A streamed answer is not part of this contract.

- A success, status 200: `{"result":"ok","content":"<text>","usage":{"input":<count>,"output":<count>},"cost":<count>}`. `content` is the text, `usage` holds the input and output tokens, and `cost` is the settled cost in micro-units.
- A refusal, status 403: `{"result":"refused","reason":R}`, where R is one of `over-cap`, `no-cap`, `not-active`, `pending`, `closing`, `no-such-account`, `credential-mismatch` and `other-arguments`, written with hyphens.

Both objects are closed, and so is `usage`: inside it an unknown key, a missing key and a repeated key are each `malformed`, so a key that a later gateway adds is `malformed`. A reader reads an answer as it reads a body: the bytes as received, so a leading EF BB BF and any byte sequence that is no UTF-8 are `malformed`, the keys in any order, JSON white space, and a repeated key is `malformed`. Any status other than 200 and 403 is read as `status`, whatever the body holds. Every other fault is read as `malformed`, among them an unknown reason and a `result` that does not fit its status.

What the device does with each answer:

- `ok`: the run goes on.
- `over-cap`: the cap is reached, and the run sends no further request.
- `no-cap`, `not-active`, `pending`, `closing`, `no-such-account` and `credential-mismatch`: the task is handed back with the account's state, and nothing is sent again.
- `other-arguments`: a fault of the sender; the task is handed back.
- No answer within the hold's life and an allowance that the branch sets: one more send with the same id and the same body, then a hand back.
- `status`, any other status: a hand back.
- `malformed` under 200 or 403: the task is handed back and nothing is sent again, because a 200 that cannot be read may have settled.

For a request whose answer it cannot read, `status` or `malformed`, the device makes no figure of its own: what it shows is what the ledger says at its next read.

The device never moves a request to another tier or host by itself.

## The files

- `body.schema.json` is the body as JSON Schema 2020-12, in the keywords that the billing run's walk admits. It is wider than the reader wherever its keywords have no word: the number of messages, because `messages` has no `minItems` or `maxItems`, the spelling of a number, a repeated key, a lone surrogate, the order of the errors, bytes that are no UTF-8, and the size of the body, because `maxLength` counts the characters of one `content` and no keyword bounds the bytes of the whole body. The reader decides.
- `vectors.jsonl` holds one body per line with the result that a reader must give: `valid`, `canon` (the bytes are what a writer writes) and `error`.
- `answers.jsonl` holds one answer per line with its status and the result that a reader must give.
- `tier-limits.tsv` holds the limits above.

A line of `vectors.jsonl` or `answers.jsonl` holds its body under `body`, as a JSON string, or under `hex` in its place, as an even number of the digits `0` to `9` and `a` to `f`, never both and never neither; `n` counts over both forms. A reader takes the bytes of whichever form the line has. The `hex` form carries bytes that a JSON string cannot, and the bodies that a reader must take from the wire and not from a decoded string. A line of either file ends at LF alone, and none holds U+0085, U+2028 or U+2029 raw, so a split at LF and Python's `str.splitlines()` give the same lines.

`cargo run -q --locked -p halvo-route-wire --bin gateway-check -- --dir docs/r6/gateway` runs every vector through the crate and prints one line with the counts. A reader on the billing side runs the same vectors through its own code and must give the same result for each line.

The check of these files is the script `scripts/r6/ci.d/20-r6-gateway.sh`: it runs the crate's tests, which pin the schema's text, the names of the vectors with their results and the edge that each edge vector holds, and then `gateway-check`, and the line of `gateway-check` alone on a copy of the files is not the check.
