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

A reader accepts the keys in any order and JSON white space between tokens, decodes escapes in names and values, and refuses a repeated key. `max_output_tokens` is digits alone: a sign, a fraction, an exponent or a value above 4,294,967,295 is refused. A writer writes the keys in the order above with no space, and writes a string as `serde_json` writes one: `\"`, `\\`, the short escapes `\b`, `\f`, `\n`, `\r` and `\t`, `\u00xx` with lower-case digits for the other characters under 0x20, and every other character as UTF-8.

A refused body gets one of these errors. Where two fit, the first of this order is the answer:

| Error | When |
|---|---|
| `malformed` | The bytes are not one JSON object of UTF-8 text, a value has the wrong JSON type, a count is not written as above, or a string holds an escape of a lone surrogate. |
| `key` | A key is unknown, missing or repeated, in the body or in a message. |
| `tier` | `tier` is not one of its two words. |
| `effort` | `effort` is not one of its three words. |
| `role` | A `role` is not one of its three words. |
| `limit` | The body holds 0 or more than 64 messages, or `max_output_tokens` is 0 or more than 2048. |
| `size` | The body is more than 131072 bytes. |

`size` comes last, so a reader that checks the length of a body before it reads the body gives another error than this order for a body that is too large and also fails a rule above it. The vectors `limit_before_size` and `malformed_before_size` hold that case.

## The limits

`tier-limits.tsv` holds the limits of each tier. Both tiers have the same values today, and all five are provisional: no hosted step has run, and the first recorded hosted steps replace them.

| Column | Value | Meaning |
|---|---|---|
| `max_body_bytes` | 131072 | The largest body in bytes. |
| `max_output_tokens` | 2048 | The largest output limit. |
| `hold_life_ms` | 120000 | The life of the hold. A device that has no answer waits this long before its second send. |
| `upstream_deadline_ms` | 90000 | The least time that must be left on a hold for a dispatch, in the billing run's meaning. The dispatch margin stays the billing run's. |
| `status` | `provisional` | No value is measured. |

The two times must fit under the limit that the billing run reports for a Worker whose client has gone.

## The two answers

The device reads two answers, each one complete body that the gateway sends once the upstream call has ended. A streamed answer is not part of this contract.

- A success, status 200: `{"result":"ok","content":"<text>","usage":{"input":<count>,"output":<count>},"cost":<count>}`. `content` is the text, `usage` holds the input and output tokens, and `cost` is the settled cost in micro-units.
- A refusal, status 403: `{"result":"refused","reason":R}`, where R is one of `over-cap`, `no-cap`, `not-active`, `pending`, `closing`, `no-such-account`, `credential-mismatch` and `other-arguments`, written with hyphens.

Both objects are closed. A reader takes their keys in any order with JSON white space between tokens and refuses a repeated key. Any status other than 200 and 403 is read as `status`, whatever the body holds. Every other fault is read as `malformed`, among them an unknown reason and a `result` that does not fit its status.

What the device does with each answer:

- `ok`: the run goes on.
- `over-cap`: the cap is reached, and the run sends no further request.
- `no-cap`, `not-active`, `pending`, `closing`, `no-such-account` and `credential-mismatch`: the task is handed back with the account's state, and nothing is sent again.
- `other-arguments`: a fault of the sender; the task is handed back.
- No answer within the hold's life: one more send with the same id and the same body, then a hand back.
- Any other status: a hand back.

The device never moves a request to another tier or host by itself.

## The files

- `body.schema.json` is the body as JSON Schema 2020-12, in the keywords that the billing run's walk admits. It is wider than the reader in two places, the number of messages and the spelling of a number, and the reader decides.
- `vectors.jsonl` holds one body per line with the result that a reader must give: `valid`, `canon` (the bytes are what a writer writes) and `error`.
- `answers.jsonl` holds one answer per line with its status and the result that a reader must give.
- `tier-limits.tsv` holds the limits above.

`cargo run -q --locked -p halvo-route-wire --bin gateway-check -- --dir docs/r6/gateway` runs every vector through the crate and prints one line with the counts. A reader on the billing side runs the same vectors through its own code and must give the same result for each line.
