# S3: unknown-key rejection on the real runtime

rule: MET (validator MET, typify MET)
rule (validator): MET. extend MET: of 18040 bodies with an unknown key Zod rejected 18040 and the Worker accepted 0; of 169 bodies for the router rule the Worker accepted 0; state gained 0 rows; the sentinel was found in 0 responses, 0 console lines and 0 state rows; the export is closed, with 0 findings at 4 object subschemas. intersection MET: of 18040 bodies with an unknown key Zod rejected 18040 and the Worker accepted 0; of 169 bodies for the router rule the Worker accepted 0; state gained 0 rows; the sentinel was found in 0 responses, 0 console lines and 0 state rows; the export is closed, with 0 findings at 4 object subschemas. The closed check flags the forbidden form allof.
rule (typify): MET. extend MET: the crate builds, 4 of 4 structs deny unknown fields, 0 flatten fields, 0 map fields, serde_json read 2 of 2 unmodified bodies and refused 96 of 96 unknown keys. intersection MET: the crate builds, 4 of 4 structs deny unknown fields, 0 flatten fields, 0 map fields, serde_json read 2 of 2 unmodified bodies and refused 96 of 96 unknown keys.
allof: forbidden by the contract, flagged by the walk. Its export holds allOf at /properties/device; the closed check reports allOf there and on its 3 allOf fixtures. Its measured values decide neither line: of 18040 bodies with an unknown key Zod rejected 18040 and the Worker accepted 0; of 169 bodies for the router rule the Worker accepted 0; state gained 0 rows; the sentinel was found in 0 responses, 0 console lines and 0 state rows; the export is not closed, with 1 findings at 5 object subschemas (/properties/device: allOf, which the contract forbids); the crate builds, 3 of 3 structs deny unknown fields, 0 flatten fields, 0 map fields, serde_json read 0 of 2 unmodified bodies and refused 96 of 96 unknown keys.
as the first statement counts, with allof as a third form: rule: MISSED (validator MISSED, typify MISSED)

These five lines are the second result: the first five lines that `sh spikes/s3/run.sh` printed
at commit dde000f, under the second statement of the rule. The run exited 0. The line that the
gate's verifier reproduces is the first, with the validator line and the typify line below it.
The command is `sh spikes/s3/run.sh`.

This file holds two statements of the rule and one result for each.

| | first | second |
|---|---|---|
| The statement | "The rule" and "What counts", written before the first run | "The second statement of the rule", written with the first result known |
| Its commit | ff16892 | 1bed9a2 |
| The result | `rule: MISSED (validator MISSED, typify MISSED)` | `rule: MET (validator MET, typify MET)` |
| Where it is | "The first result", from its three rule lines to "Reproduce" | the five lines above, and "The second result" to the end of the file |
| Measured at | c4c3230, recorded in 7d0c260 | dde000f |
| The command | `sh spikes/s3/run.sh` in a checkout of 7d0c260, exit 1 | `sh spikes/s3/run.sh` at this commit, exit 0 |

The measurement is the same in both results. Zod rejected every unknown key in all three forms
both times, and the fifth line above shows that the form `allof` still misses both lines as the
first statement counts it. The rule changed. After the first result the coordinator of the run
ruled that the contract forbids `allOf` in an exported schema. Under the second statement the
lines are decided on `extend` and `intersection`, and `allof` is the control that the closed
check must flag. The second statement was written after the first result was read, so its MET
is not a prediction that came true.

## The first result

From the line below that starts with `rule: MISSED` to the end of "What counts", this file is
what commit 7d0c260 recorded, byte for byte: the first result, then the first statement. Where
that text says "here", "above" or "this file", it speaks of the first result and of the code at
c4c3230.

rule: MISSED (validator MISSED, typify MISSED)
rule (validator): MISSED. extend MET: of 18040 bodies with an unknown key Zod rejected 18040 and the Worker accepted 0; of 169 bodies for the router rule the Worker accepted 0; state gained 0 rows; the sentinel was found in 0 responses, 0 console lines and 0 state rows; the export is closed, with 0 findings at 4 object subschemas. intersection MET: of 18040 bodies with an unknown key Zod rejected 18040 and the Worker accepted 0; of 169 bodies for the router rule the Worker accepted 0; state gained 0 rows; the sentinel was found in 0 responses, 0 console lines and 0 state rows; the export is closed, with 0 findings at 4 object subschemas. allof MISSED: of 18040 bodies with an unknown key Zod rejected 18040 and the Worker accepted 0; of 169 bodies for the router rule the Worker accepted 0; state gained 0 rows; the sentinel was found in 0 responses, 0 console lines and 0 state rows; the export is not closed, with 1 findings at 5 object subschemas (/properties/device: allOf of objects without unevaluatedProperties false).
rule (typify): MISSED. extend MET: the crate builds, 4 of 4 structs deny unknown fields, 0 flatten fields, 0 map fields, serde_json read 2 of 2 unmodified bodies and refused 96 of 96 unknown keys. intersection MET: the crate builds, 4 of 4 structs deny unknown fields, 0 flatten fields, 0 map fields, serde_json read 2 of 2 unmodified bodies and refused 96 of 96 unknown keys. allof MISSED: the crate builds, 3 of 3 structs deny unknown fields, 0 flatten fields, 0 map fields, serde_json read 0 of 2 unmodified bodies and refused 96 of 96 unknown keys.

Only the form `allof` misses: an intersection of two strict objects whose export keeps `allOf`.
Zod rejected every unknown key in it, as in the other two forms. Its export is `allOf` of two
closed objects, which no body that holds the keys of both halves can satisfy: the closed check
reports it, the jsonschema crate finds 0 of 2 unmodified bodies valid against it, and the types
that typify generates from it read 0 of 2. The forms `extend` and `intersection` meet both
lines. Zod folds that plain intersection into one object, so its export is byte-identical to the
extended one.

Measured 2026-11-10 (UTC) on an Apple M3 Max with 64 GiB, macOS 27.2 and Node v26.12.0, at commit
c4c3230.

## Measured values

What `sh spikes/s3/run.sh` prints below its three rule lines:

versions: zod 4.6.7, @cloudflare/vitest-plugin 1.6.1, workerd 1.20261109.1, vitest 5.1.0, fast-check 4.10.2, typify 0.8.0, jsonschema 0.61.0, cargo 1.99.1 (5f94df478 2026-08-27)
runtime: user agent Cloudflare-Workers; Zod's allowsEval false; new Function allowed; Object.prototype has 12 own names before the cases and 12 after; "polluted" in {} is false
seed of the arbitrary names: 20261109; sentinel occurrences in the whole log of the test run: 0
exports: extend holds no composition keyword; intersection holds no composition keyword; allof holds allOf; extend and intersection are byte-identical; extend and allof are different; intersection and allof are different

### extend

Unmodified bodies: 2 of 2 accepted, 2 rows in state. Arbitrary names: 1000 sampled, 898 distinct and sent, the longest 23 UTF-16 units. After all cases state holds 2 rows; the dump read 1 instances of the Durable Object. The Worker wrote 18211 console lines, 4 distinct.

| unknown key, one per body | bodies at root / cap / cap.period / device | accepted | rejected by the validator | rejected by the router rule | not JSON | other |
|---|---|---|---|---|---|---|
| forbidden names | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| Object.prototype names | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| __proto__, constructor, prototype with an object value | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of declared keys | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of forbidden names | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| arbitrary names | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |

| router rule and body shape | bodies | accepted | rejected by the router rule | not JSON | rejected by the validator | other | parsed value valid | accepted with the rule off (K3) |
|---|---|---|---|---|---|---|---|---|
| repeat, sentinel first | 23 | 0 | 23 | 0 | 0 | 0 | 23 | 23 |
| repeat, sentinel last | 23 | 0 | 23 | 0 | 0 | 0 | 0 | 0 |
| padding | 26 | 0 | 26 | 0 | 0 | 0 | 26 | 26 |
| trailing data | 14 | 0 | 0 | 14 | 0 | 0 | 0 | 0 |
| encoding | 67 | 0 | 55 | 12 | 0 | 0 | 52 | 52 |
| not an object | 16 | 0 | 0 | 2 | 14 | 0 | 0 | 0 |

Export: equal to the committed file: true; 4 object subschemas, 0 without additionalProperties false, 0 findings. In output mode the export of z.object at cap has 0 findings at cap.
typify: 400 generated lines; structs Request, RequestCap, RequestCapPeriod, RequestDevice; enums RequestCapCurrency, RequestCapPeriodUnit, RequestDevicePlatform, RequestLocale, RequestTier; tuple structs RequestDeviceInstallId, ConversionError; aliases 0. The jsonschema crate on the same export: 2 of 2 unmodified bodies valid, 96 of 96 unknown keys refused. serde_json wrote 2 unmodified bodies back from the generated types: 2 texts equal JSON.stringify of their parsed value and 2 hold the value they were read from.

### intersection

Unmodified bodies: 2 of 2 accepted, 2 rows in state. Arbitrary names: 1000 sampled, 898 distinct and sent, the longest 23 UTF-16 units. After all cases state holds 2 rows; the dump read 11 instances of the Durable Object. The Worker wrote 18211 console lines, 4 distinct.

| unknown key, one per body | bodies at root / cap / cap.period / device | accepted | rejected by the validator | rejected by the router rule | not JSON | other |
|---|---|---|---|---|---|---|
| forbidden names | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| Object.prototype names | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| __proto__, constructor, prototype with an object value | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of declared keys | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of forbidden names | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| arbitrary names | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |

| router rule and body shape | bodies | accepted | rejected by the router rule | not JSON | rejected by the validator | other | parsed value valid | accepted with the rule off (K3) |
|---|---|---|---|---|---|---|---|---|
| repeat, sentinel first | 23 | 0 | 23 | 0 | 0 | 0 | 23 | 23 |
| repeat, sentinel last | 23 | 0 | 23 | 0 | 0 | 0 | 0 | 0 |
| padding | 26 | 0 | 26 | 0 | 0 | 0 | 26 | 26 |
| trailing data | 14 | 0 | 0 | 14 | 0 | 0 | 0 | 0 |
| encoding | 67 | 0 | 55 | 12 | 0 | 0 | 52 | 52 |
| not an object | 16 | 0 | 0 | 2 | 14 | 0 | 0 | 0 |

Export: equal to the committed file: true; 4 object subschemas, 0 without additionalProperties false, 0 findings. In output mode the export of z.object at cap has 0 findings at cap.
typify: 400 generated lines; structs Request, RequestCap, RequestCapPeriod, RequestDevice; enums RequestCapCurrency, RequestCapPeriodUnit, RequestDevicePlatform, RequestLocale, RequestTier; tuple structs RequestDeviceInstallId, ConversionError; aliases 0. The jsonschema crate on the same export: 2 of 2 unmodified bodies valid, 96 of 96 unknown keys refused. serde_json wrote 2 unmodified bodies back from the generated types: 2 texts equal JSON.stringify of their parsed value and 2 hold the value they were read from.

### allof

Unmodified bodies: 2 of 2 accepted, 2 rows in state. Arbitrary names: 1000 sampled, 898 distinct and sent, the longest 23 UTF-16 units. After all cases state holds 2 rows; the dump read 21 instances of the Durable Object. The Worker wrote 18211 console lines, 4 distinct.

| unknown key, one per body | bodies at root / cap / cap.period / device | accepted | rejected by the validator | rejected by the router rule | not JSON | other |
|---|---|---|---|---|---|---|
| forbidden names | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| Object.prototype names | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| __proto__, constructor, prototype with an object value | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of declared keys | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of forbidden names | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| arbitrary names | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |

| router rule and body shape | bodies | accepted | rejected by the router rule | not JSON | rejected by the validator | other | parsed value valid | accepted with the rule off (K3) |
|---|---|---|---|---|---|---|---|---|
| repeat, sentinel first | 23 | 0 | 23 | 0 | 0 | 0 | 23 | 23 |
| repeat, sentinel last | 23 | 0 | 23 | 0 | 0 | 0 | 0 | 0 |
| padding | 26 | 0 | 26 | 0 | 0 | 0 | 26 | 26 |
| trailing data | 14 | 0 | 0 | 14 | 0 | 0 | 0 | 0 |
| encoding | 67 | 0 | 55 | 12 | 0 | 0 | 52 | 52 |
| not an object | 16 | 0 | 0 | 2 | 14 | 0 | 0 | 0 |

Export: equal to the committed file: true; 5 object subschemas, 0 without additionalProperties false, 1 findings (/properties/device: allOf of objects without unevaluatedProperties false). In output mode the export of z.object at cap has 0 findings at cap.
typify: 297 generated lines; structs Request, RequestCap, RequestCapPeriod; enums RequestCapCurrency, RequestCapPeriodUnit, RequestDevice, RequestLocale, RequestTier; tuple structs ConversionError; aliases 0. The jsonschema crate on the same export: 0 of 2 unmodified bodies valid, 96 of 96 unknown keys refused. serde_json wrote 0 unmodified bodies back from the generated types: 0 texts equal JSON.stringify of their parsed value and 0 hold the value they were read from.

### observations outside the rule

A probe with what the unit's schema does not hold: a discriminated union of three strict objects, one of which holds only the tag, a nullable enum and an array of strict objects (buildProbe in src/schema.ts).

- Zod under workerd, called without the Worker: accepted 3 of 3 probe bodies and refused 27 of 27 bodies with an unknown key, at the positions root, choice, items.[].
- Export: equal to the committed file: true; composition keywords in it: anyOf, oneOf; findings of the closed check: 0.
- typify: the crate builds; structs and struct variants Request, RequestChoice::Count, RequestChoice::Unit, RequestItemsItem; of them 0 do not deny unknown fields; 0 flatten fields; 0 map fields; serde_json read 3 of 3 probe bodies and refused 96 of 108 unknown keys; it read an unknown key at: /choice; the jsonschema crate found 3 of 3 bodies valid and refused 108 of 108.

### controls

- K1 extend: caught. z.object at one position: accepted at root / cap / cap.period / device when that position is open: 32 of 32 / 32 of 32 / 32 of 32 / 32 of 32; accepted at the other positions, beyond what the schema under test accepts there: 0.
- K2 extend: caught. z.looseObject at one position: state rows that hold the sentinel: 32 / 32 / 32 / 32.
- K3 extend: caught. router rule off: accepted, of the bodies whose parsed value is valid, 23 of 23 in "repeat, sentinel first", 0 of 0 in "repeat, sentinel last", 26 of 26 in "padding", 0 of 0 in "trailing data", 52 of 52 in "encoding", 0 of 0 in "not an object".
- K4 extend: caught. a handler that logs the body inside an object and returns it: the sentinel in 128 of 128 rejected responses and in 128 console lines.
- K5 extend: caught. the closed check on the export of each K1 and K2 schema: findings at the changed position, strip / loose: 1 / 3, 1 / 3, 1 / 3, 1 / 3.
- K1 intersection: caught. z.object at one position: accepted at root / cap / cap.period / device when that position is open: 32 of 32 / 32 of 32 / 32 of 32 / 32 of 32; accepted at the other positions, beyond what the schema under test accepts there: 0.
- K2 intersection: caught. z.looseObject at one position: state rows that hold the sentinel: 32 / 32 / 32 / 32.
- K3 intersection: caught. router rule off: accepted, of the bodies whose parsed value is valid, 23 of 23 in "repeat, sentinel first", 0 of 0 in "repeat, sentinel last", 26 of 26 in "padding", 0 of 0 in "trailing data", 52 of 52 in "encoding", 0 of 0 in "not an object".
- K4 intersection: caught. a handler that logs the body inside an object and returns it: the sentinel in 128 of 128 rejected responses and in 128 console lines.
- K5 intersection: caught. the closed check on the export of each K1 and K2 schema: findings at the changed position, strip / loose: 1 / 3, 1 / 3, 1 / 3, 1 / 1.
- K1 allof: caught. z.object at one position: accepted at root / cap / cap.period / device when that position is open: 32 of 32 / 32 of 32 / 32 of 32 / 32 of 32; accepted at the other positions, beyond what the schema under test accepts there: 0.
- K2 allof: caught. z.looseObject at one position: state rows that hold the sentinel: 32 / 32 / 32 / 32.
- K3 allof: caught. router rule off: accepted, of the bodies whose parsed value is valid, 23 of 23 in "repeat, sentinel first", 0 of 0 in "repeat, sentinel last", 26 of 26 in "padding", 0 of 0 in "trailing data", 52 of 52 in "encoding", 0 of 0 in "not an object".
- K4 allof: caught. a handler that logs the body inside an object and returns it: the sentinel in 128 of 128 rejected responses and in 128 console lines.
- K5 allof: caught. the closed check on the export of each K1 and K2 schema: findings at the changed position, strip / loose: 1 / 3, 1 / 3, 1 / 3, 3 / 7.
- K6 open object: caught. typify on an export with z.object at cap: structs that do not deny unknown fields: RequestCap; serde_json read 24 of 96 unknown keys.
- K6 map: caught. typify on a schema-valued additionalProperties: map fields: Request.labels: HashMap; serde_json read 3 of 24 unknown keys.

## How each value was measured, and what limits it

- **Bodies and what happened to them.** Each body is one POST to the Worker through `SELF.fetch`
  of `@cloudflare/vitest-plugin`, inside workerd. The response decides the column: a 2xx status is
  accepted; a 400 with one of three fixed bodies names the layer that rejected it; anything else
  is "other". Limits: one schema with four object positions and two unmodified bodies; strings
  are an enum, a constant or a pattern, integers have bounds, one key is optional. The requests
  do not cross a socket, so nothing here covers HTTP framing, a content type or a size limit.
- **Unknown keys.** One key per body. The test asserts that each body is byte-equal to
  JSON.stringify of its parsed value before it sends it, so the router rule lets it through and
  only Zod can reject it; the judge does not count a run in which one of these bodies was stopped
  before the validator. A body with many unknown keys was not measured (see the report: a review
  found that Zod needs quadratic time for such a body in an intersection).
- **Arbitrary names.** `fc.sample` of fast-check 4.10.2 with seed 20261109 drew 1000 strings from
  three string arbitraries (the default, grapheme and binary units). 898 are distinct and each
  was sent once per position, body and place; the longest has 23 UTF-16 units. It is a sample:
  a name that was not drawn was not tried.
- **Variants.** 23 rewrites of each name: upper case, a capital first letter, alternating case,
  fullwidth letters, a Cyrillic lookalike, the Kelvin sign, the long s, the dotless and the
  dotted i, a zero-width space before and after, a byte order mark before, a zero-width joiner
  and a soft hyphen inside, a combining accent, a space before and after, a tab, a newline and a
  NUL after, a hyphen for each underscore, no underscore, and camel case. It is a list, not all
  of Unicode.
- **The runtime.** workerd 1.20261109.1 with compatibility date 2026-11-01, as the vitest plugin
  starts it. Zod reported `allowsEval` false, so it validated without generated code. Zod decides
  that from the user agent, which a deployed Worker shares. `new Function` works in the test
  isolate because the plugin turns it on; the path with generated code (Node) was not measured,
  and neither was Cloudflare's network.
- **State.** Every binding is emptied before a form's cases. The dump reads every instance of
  the Durable Object: every row of every table an instance can read and every key of its
  key-value storage, with bytes turned into text. It is read after the unmodified bodies and
  after all cases, not after each case. The Worker only inserts rows, so 0 rows added over the
  run means that no case added one.
- **Sentinel.** A substring search in every response (status, status text, headers, body), in
  every console line (spies on console.log, info, warn, error and debug in the Worker's isolate;
  an object argument is written as JSON first), in the state dump and in the whole log of the
  test run. A sentinel that something had encoded or split would not be found.
- **Export.** `z.toJSONSchema(schema, { io: "input" })` runs inside workerd, and its text must
  equal the committed file byte for byte. `src/closed.ts` walks it. Limits: the walk is this
  spike's own code, checked on nine small fixtures in the same run. A review showed schemas that
  it passes although they admit an unknown key: `not`, `if` and `then`, an array without `items`
  or with `items: true`, `prefixItems` alone, `contains`, a `$ref` outside `$defs`. None of them
  is in these exports. The jsonschema crate 0.61.0 read the same exports as a cross-check outside
  the rule; its counts are in the typify line of each form.
- **typify.** The build script of `spikes/s3/crate` runs typify 0.8.0 on one committed export;
  `cargo run --locked --offline` builds and runs the checker, once per export. The struct, flatten
  and map counts come from the generated source, parsed with syn 2. The behavior test gives
  serde_json 3 unknown keys (`window_title`, `__proto__`, `x`), each with 4 kinds of value, at
  each of the 4 object positions of the 2 unmodified bodies: 96 cases. Limits: the struct count
  covers structs and struct variants with named fields; the behavior test reaches only the
  objects that a committed body holds. The generated types are wider than the schema in values:
  the constant `kind` is a String, `amount_minor` is an i64 and `count` a NonZeroU64.
- **What serde_json writes.** An observation outside the rule. The checker wrote the unmodified
  bodies back from the generated types, with the keys in alphabetical order. For `extend` and
  `intersection` both texts equal JSON.stringify of their parsed value, compared under Node and
  not under workerd. A float would not: serde_json writes `1.0` where JSON.stringify writes `1`.
- **The probe.** Outside the rule. For a discriminated union typify writes one enum with
  `deny_unknown_fields`, but a member that holds only the tag becomes a unit variant, and
  serde_json reads unknown keys there: it refused 96 of 108 keys and read the 12 at `/choice` of
  the body that picks that member. Zod and the jsonschema crate refused all of them.
- **Runs.** `sh spikes/s3/run.sh` ran three times in the worktree at commit c4c3230 and twice in a fresh clone
  of the branch after `sh scripts/prerequisites.sh`. All five runs printed the same text and
  exited 1. The three runs in the worktree took 115, 232 and 145 s of wall time, read from `date`
  around each run, while other jobs loaded the machine (load average above 100). On a quieter
  machine the test part of one run took 23 s by vitest's own figure.

## Where the run differs from what this file stated before it

1. Three forms, not two. The rule commit calls the intersection "the form that exports as
   `allOf`". Zod 4.6.7 folds an intersection of two plain strict objects into one object, so that
   form exports the same bytes as the extended object. The runs made before the review measured
   those two forms only and read MET on both lines. A reviewer showed that the form the rule
   commit describes had not been measured. The form `allof` was added then: the same intersection
   with a description on one half, which stops the fold. The lines above count all three forms,
   and the form `allof` is the only one that misses.
2. State is compared once per run, not after every case (see State above).
3. The judge is stricter than "What counts" in four places: a response that is neither 2xx nor a
   fixed rejection fails the validator line, a change to `Object.prototype` fails it, the
   sentinel anywhere in the log of the test run fails it, and a run in which a class of cases has
   no body, or in which an unknown-key body did not reach the validator, does not count.
4. Controls K1, K2 and K4 send the eight forbidden names, 32 bodies per position, not every
   unknown-key case. K1 and K2 compare the other positions with what the schema under test
   accepts there, which is 0 here. K4 counts the responses that the leaky handler rejects, which
   is all of them here.
5. The arbitrary names are 898 distinct strings of the 1000 sampled, each sent once.
6. Four things were added after the rule commit and decide no rule line: the whole-run control,
   the jsonschema cross-check, the serde_json write-back and the probe.

## The whole-run control

`node spikes/s3/mutation-control.mjs` copies the committed tree into a scratch directory and
changes one thing at a time. Each change must turn the verdict of the forms it touches and of no
other form, and a run without arbitrary names must not count. This is the failing test that D13
starts from. Its output:

```
mutation-control: a copy of c4c3230 is in .tmp/s3-mutation.dbY45a
as required: the unchanged copy: run.sh exited 1; validator: extend MET, intersection MET, allof MISSED; typify: extend MET, intersection MET, allof MISSED
as required: one z.strictObject changed to z.object, then exported again: run.sh exited 1; validator: extend MISSED, intersection MET, allof MISSED; typify: extend MISSED, intersection MET, allof MISSED
as required: the router rule removed: run.sh exited 1; validator: extend MISSED, intersection MISSED, allof MISSED; typify: extend MET, intersection MET, allof MISSED
as required: every rejection logs and returns the body: run.sh exited 1; validator: extend MISSED, intersection MISSED, allof MISSED; typify: extend MET, intersection MET, allof MISSED
as required: a rejection also logs the body inside an object, the answer stays fixed: run.sh exited 1; validator: extend MISSED, intersection MISSED, allof MISSED; typify: extend MET, intersection MET, allof MISSED
as required: a rejection also returns the body in a header, the body of the answer stays fixed: run.sh exited 1; validator: extend MISSED, intersection MISSED, allof MISSED; typify: extend MET, intersection MET, allof MISSED
as required: a rejected body is written to another instance of the Durable Object: run.sh exited 1; validator: extend MISSED, intersection MISSED, allof MISSED; typify: extend MET, intersection MET, allof MISSED
as required: the committed export opened at cap, the schema unchanged: run.sh exited 1; validator: extend MISSED, intersection MET, allof MISSED; typify: extend MISSED, intersection MET, allof MISSED
as required: no arbitrary name is sampled: run.sh exited 2 and printed "rule: not decided. The run does not count: extend: no body of "arbitrary names" at root; extend: no body of "arbitrary names" at cap; extend: no body "
```

Its first run found a defect in `judge.mjs`, fixed in the commit "Add the whole-run control of
spike S3": with a schema that accepts some cases, control K4 reported the run as not counting.

## Reproduce

```sh
sh scripts/prerequisites.sh           # once; the only step that uses the network
sh spikes/s3/run.sh                   # prints the three rule lines and the measured values
node spikes/s3/mutation-control.mjs   # the whole-run control
```

`run.sh` exits 0 when both lines read MET, 1 when one reads MISSED, as it does here, and 2 when
the run does not count. It writes under `.tmp/` only, starts from an empty state and is safe to
run twice. After a change to `src/schema.ts`, `node spikes/s3/export.mjs` writes the exports again.

The two sections below are the text of the rule commit, unchanged.

## The rule

The unit, as planned (plan.md of run 20261109-halvo-r7-billing, unit S3):

> S3 unknown-key rejection on the real runtime. Under workerd through the vitest plugin, a
> three-level z.strictObject schema with one composed object receives forbidden names, arbitrary
> names, __proto__, constructor, toString and case and Unicode variants at every object position,
> and a repeated schema key whose first value holds a sentinel. The router rule under test: a body
> that is not byte-equal to JSON.stringify of its parsed value is rejected, which covers repeated
> keys, padding and alternative encodings. The exported JSON Schema is checked closed at every
> object; typify 0.8.0 generates Rust types from that export into a scratch crate, built with
> --locked --offline (cargo 1.99.1 in ~/.cargo/bin). It meets its rule when nothing is accepted, no
> sentinel reaches state or output, the crate builds, every struct denies unknown fields and no
> type has a map field. It unblocks D12 and gives D13 its first failing test. ArkType 2.2.9, then
> TypeBox with Ajv standalone, run only if Zod misses on an unknown key; if typify misses, the app
> uses hand-written serde types checked against the schema by the jsonschema crate (r4#2).

The plan's gate reads this rule as two lines. This file reports both under the first line, and
the first line reads MET only when both read MET.

- validator: nothing accepted, no sentinel in state or output and the export closed at every
  object, for the validator the spike ends with.
- typify: the crate builds, every struct denies unknown fields and no type has a map field.

## What counts

These definitions were fixed before any code of the spike ran.

**Setup.** One Worker (`spikes/s3/src`) runs under workerd through `@cloudflare/vitest-plugin`.
Its route reads the body as bytes, applies the router rule, then validates with Zod 4.6.7. An
accepted request writes the validated value to the storage of a Durable Object, logs one fixed
line and answers 200 with a fixed body. A rejected request logs one fixed line and answers 400
with a fixed body that names the rule which rejected it.

**Schema.** Three nested `z.strictObject` levels (the root, `cap`, `cap.period`) and one composed
object (`device`). The composed object is built in two forms, each a schema of its own: `extend`
(a strict object extended with more keys) and `intersection` (`z.intersection` of two strict
objects, the form that exports as `allOf`, r4#65). Every measurement runs on both forms, and a
rule line reads MET only if it holds for both. A form whose unmodified body the validator rejects
cannot be measured: the file reports it as unusable, and the line is decided on the other form.

**Cases.** A case is one request body. The object positions are the four objects of the
unmodified body: the root, `cap`, `cap.period` and `device`.

1. Unknown keys. One key per body, inserted at each position as the first key and as the last
   key, with the sentinel as its value. The body is byte-equal to JSON.stringify of its parsed
   value, so only the validator can reject it. The names:
   - forbidden names: the eight modality names of researcher-4 (screen_frame, keystroke, pointer,
     foreground_app, window_title, system_audio, microphone, camera);
   - `__proto__`, `constructor`, `toString` and every other own property name of
     `Object.prototype` on the runtime; `__proto__`, `constructor` and `prototype` also with an
     object value that holds declared keys with the sentinel;
   - case and Unicode variants of each declared key of the position and of each forbidden name;
   - arbitrary names: 1000 strings sampled by fast-check with a fixed seed that the run prints.
2. The router rule. For each position and each declared key, the key twice, with the sentinel as
   the first value and the valid value last, and in the other order. Padding: whitespace before,
   after and inside the body, and a UTF-8 byte order mark. Alternative encodings of the same
   value: escaped keys and strings, other number forms, UTF-16, invalid UTF-8 and a gzip body.
3. Bodies that are not an object: a top-level array, string, number and null.

**Accepted.** A case is accepted when its response status is 2xx, or when state holds more rows
after it than before.

**Sentinel.** A fixed synthetic string. It reaches output when it appears in a response (status
text, headers or body) or in a line the Worker writes to the console. It reaches state when it
appears in the dump of the Durable Object's storage.

**Export.** `z.toJSONSchema(schema, { io: "input" })`, as unit 2.1 plans it. Closed at every
object means: every subschema that describes an object carries `additionalProperties: false`;
where a schema composes others with `allOf`, the composing schema carries
`unevaluatedProperties: false`, because `additionalProperties` does not see across branches
(r4#65); no object uses `patternProperties`, `propertyNames` or a schema-valued
`additionalProperties`. The file also reports the plain count of object subschemas without
`additionalProperties: false`. The export made under workerd must equal the committed file that
typify reads.

**typify.** typify 0.8.0 generates the types in the build script of `spikes/s3/crate` from the
committed export, and `cargo build --locked --offline` builds them with cargo 1.99.1. "Every
struct denies unknown fields" means: every generated struct with named fields carries
`#[serde(deny_unknown_fields)]` and no field carries `#[serde(flatten)]`, both read from the
generated source with syn; and serde_json reads the unmodified body into the root type and
refuses a body with an unknown key at each object position. "No type has a map field" means: no
generated field or alias names HashMap, BTreeMap, IndexMap, serde_json::Map or serde_json::Value.

**Controls.** The positive control: the unmodified body is accepted and written to state, for
each form. Each control below must fail the measurement it guards. If one does not, the result
does not count and this file says so.

- K1. At each object position in turn, the schema with that position's strict object changed to
  `z.object`. The unknown-key cases must be accepted at that position, and only there.
- K2. The same with `z.looseObject`. The sentinel must be found in state.
- K3. The router rule switched off. The repeated-key, padding and encoding bodies whose parsed
  value is valid must be accepted.
- K4. A rejection handler that logs and returns the body. The sentinel must be found in the
  console lines and in the responses.
- K5. The closed check on the export of each K1 and K2 schema must name the changed position.
- K6. typify on an open object schema and on a schema-valued `additionalProperties`. The struct
  check must report a struct that does not deny unknown fields, serde_json must read an unknown
  key into it, and the map check must report a map field.

**Fallbacks.** ArkType 2.2.9, then TypeBox with Ajv standalone, run only if Zod accepts an
unknown key. If the typify line reads MISSED, the app uses hand-written serde types checked
against the schema by the jsonschema crate (r4#2); this spike does not build that path.

## The second statement of the rule

Written on 2026-11-10 (UTC), after the first result and before the rerun. Everything above this
section is the file as commit 7d0c2609c553b2351615d426caa968e3eb53c4dd recorded it, and stays as
it is. That commit holds the first result, measured at commit c4c3230. It reads `rule: MISSED
(validator MISSED, typify MISSED)`.

This statement was written with that result known. It is not a prediction. If the rerun measures
what the first runs measured, both lines below read MET, and that was known when this text was
written. What the rerun can still show is whether the measurement repeats, and whether the
whole-run control turns the lines when the closed check stops reporting `allOf`.

**What the first result decided.** Zod rejected every unknown key in all three forms. The forms
`extend` and `intersection` met both lines. The form `allof` missed both through its export
alone: an `allOf` of two closed objects, which no body with the keys of both halves satisfies.
The coordinator of the run read that result and ruled that the contract forbids `allOf` in an
exported schema, that schemas are composed by extension, and that the closed-schema walk must
fail on any `allOf`. Under that ruling `allof` is not a form the contract could use. It is the
control that the walk must flag.

**The forms.** The contract allows two: `extend`, and `intersection`, the plain intersection
that Zod folds into one object. It forbids one: `allof`, the intersection whose export holds
`allOf`. An allowed form gets no pass by its name: if its export holds `allOf`, the walk reports
it and the form misses. Both allowed forms must be usable. An allowed form whose unmodified
bodies the validator rejects misses both lines, where the first statement decided the line on
the other form.

**Closed at every object, restated.** Every subschema that describes an object carries
`additionalProperties: false`. No object uses `patternProperties`, `propertyNames` or a
schema-valued `additionalProperties`. No schema holds `allOf`: the walk reports a finding at
every schema with that keyword, whatever its branches describe. The first statement let an
`allOf` pass when the composing schema carried `unevaluatedProperties: false`, and the walk let
an object pass that carried it in place of `additionalProperties: false`. Neither passes now.

**The lines.**

1. The validator line reads MET when both of these hold, and MISSED otherwise:
   - for every allowed form, nothing is accepted, no sentinel reaches state or output and the
     export is closed at every object;
   - the walk flags the forbidden form, as point 3 defines it.
2. The typify line reads MET when, for every allowed form, the crate builds, every struct denies
   unknown fields and no type has a map field. Otherwise it reads MISSED.
3. The form `allof` is reported on a line of its own. It reads `allof: forbidden by the
   contract, flagged by the walk` or `allof: forbidden by the contract, not flagged`, and "not
   flagged" turns the validator line to MISSED. The form is flagged when all of these hold:
   - its export made under workerd equals the committed file, and that file holds `allOf`;
   - the walk, run under workerd on that export, reports a finding for `allOf` at every schema
     of the committed file that holds `allOf`. The judge finds those schemas by itself;
   - the walk reports a finding for `allOf` on three small schemas of its own: an `allOf` of
     two closed objects, the same with `unevaluatedProperties: false`, and an `allOf` of two
     strings.
4. The first line reads `rule: MET` only when the validator line and the typify line both read
   MET. `sh spikes/s3/run.sh` then exits 0. It exits 1 when a line reads MISSED and 2 when the
   run does not count.

**What stays from the first statement.** The setup, the cases, "accepted", "sentinel", the
typify definitions, the controls K1 to K6 and the fallbacks stay as "What counts" has them. For
one form, MET and MISSED mean what they meant. The form `allof` is still measured in full, and
its values are still printed. They decide neither line. A run in which a class of its cases has
no body, or in which one of its controls is not caught, still does not count.

**What the run also prints.** One line says what the lines read when the forms are counted as
the first statement counted them, with `allof` as a third form. A miss under the first
statement stays visible in every run.

**The whole-run control.** `node spikes/s3/mutation-control.mjs` gets three changes to make, one
at a time, for what this statement adds:

- a closed check that no longer reports `allOf` must turn the validator line to MISSED, with
  `allof` reported as "not flagged" and both allowed forms still MET;
- a closed check that lets an `allOf` with `unevaluatedProperties: false` pass must do the same;
- an allowed form whose export holds `allOf` must miss both lines.

The judge, the closed check and the whole-run control are changed to this statement in the
commits after this one, and the rerun follows them.

## The second result

Measured 2026-11-10 (UTC) at commit dde000f, on the machine of the first result: an Apple M3 Max
with 64 GiB, macOS 27.2 and Node v26.12.0. Its five rule lines are at the top of this file.

Both lines read MET: the two allowed forms meet them, and the closed check reports the `allOf`
of the forbidden form. The values are those of the first result. Of the 107 lines of measured
values below, 106 are the same text as in the first result, compared with `diff`. The one that
differs is the reason of the single finding in the export of `allof`. It read "allOf of objects
without unevaluatedProperties false" and reads "allOf, which the contract forbids". No count
differs.

The spike ran under the second statement in two rounds. The first round was at commit f25e0fe
and read the same rule line. Commit 983c05a recorded it. A review of the code then found that
the judge took the closed check's word alone for the allowed forms. An `allOf` at a place that
the closed check does not follow, such as list-form `items`, would have read MET. No committed
export holds one. The judge now searches the committed exports for `allOf` by itself (commit
ee7f450), and the whole-run control got four more changes (commit dde000f). The second statement
was not changed. Everything below is from the second round, at dde000f.

## Measured values of the second result

What `sh spikes/s3/run.sh` prints below its five lines:

versions: zod 4.6.7, @cloudflare/vitest-plugin 1.6.1, workerd 1.20261109.1, vitest 5.1.0, fast-check 4.10.2, typify 0.8.0, jsonschema 0.61.0, cargo 1.99.1 (5f94df478 2026-08-27)
runtime: user agent Cloudflare-Workers; Zod's allowsEval false; new Function allowed; Object.prototype has 12 own names before the cases and 12 after; "polluted" in {} is false
seed of the arbitrary names: 20261109; sentinel occurrences in the whole log of the test run: 0
exports: extend holds no composition keyword; intersection holds no composition keyword; allof holds allOf; extend and intersection are byte-identical; extend and allof are different; intersection and allof are different

### extend

Unmodified bodies: 2 of 2 accepted, 2 rows in state. Arbitrary names: 1000 sampled, 898 distinct and sent, the longest 23 UTF-16 units. After all cases state holds 2 rows; the dump read 1 instances of the Durable Object. The Worker wrote 18211 console lines, 4 distinct.

| unknown key, one per body | bodies at root / cap / cap.period / device | accepted | rejected by the validator | rejected by the router rule | not JSON | other |
|---|---|---|---|---|---|---|
| forbidden names | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| Object.prototype names | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| __proto__, constructor, prototype with an object value | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of declared keys | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of forbidden names | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| arbitrary names | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |

| router rule and body shape | bodies | accepted | rejected by the router rule | not JSON | rejected by the validator | other | parsed value valid | accepted with the rule off (K3) |
|---|---|---|---|---|---|---|---|---|
| repeat, sentinel first | 23 | 0 | 23 | 0 | 0 | 0 | 23 | 23 |
| repeat, sentinel last | 23 | 0 | 23 | 0 | 0 | 0 | 0 | 0 |
| padding | 26 | 0 | 26 | 0 | 0 | 0 | 26 | 26 |
| trailing data | 14 | 0 | 0 | 14 | 0 | 0 | 0 | 0 |
| encoding | 67 | 0 | 55 | 12 | 0 | 0 | 52 | 52 |
| not an object | 16 | 0 | 0 | 2 | 14 | 0 | 0 | 0 |

Export: equal to the committed file: true; 4 object subschemas, 0 without additionalProperties false, 0 findings. In output mode the export of z.object at cap has 0 findings at cap.
typify: 400 generated lines; structs Request, RequestCap, RequestCapPeriod, RequestDevice; enums RequestCapCurrency, RequestCapPeriodUnit, RequestDevicePlatform, RequestLocale, RequestTier; tuple structs RequestDeviceInstallId, ConversionError; aliases 0. The jsonschema crate on the same export: 2 of 2 unmodified bodies valid, 96 of 96 unknown keys refused. serde_json wrote 2 unmodified bodies back from the generated types: 2 texts equal JSON.stringify of their parsed value and 2 hold the value they were read from.

### intersection

Unmodified bodies: 2 of 2 accepted, 2 rows in state. Arbitrary names: 1000 sampled, 898 distinct and sent, the longest 23 UTF-16 units. After all cases state holds 2 rows; the dump read 11 instances of the Durable Object. The Worker wrote 18211 console lines, 4 distinct.

| unknown key, one per body | bodies at root / cap / cap.period / device | accepted | rejected by the validator | rejected by the router rule | not JSON | other |
|---|---|---|---|---|---|---|
| forbidden names | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| Object.prototype names | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| __proto__, constructor, prototype with an object value | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of declared keys | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of forbidden names | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| arbitrary names | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |

| router rule and body shape | bodies | accepted | rejected by the router rule | not JSON | rejected by the validator | other | parsed value valid | accepted with the rule off (K3) |
|---|---|---|---|---|---|---|---|---|
| repeat, sentinel first | 23 | 0 | 23 | 0 | 0 | 0 | 23 | 23 |
| repeat, sentinel last | 23 | 0 | 23 | 0 | 0 | 0 | 0 | 0 |
| padding | 26 | 0 | 26 | 0 | 0 | 0 | 26 | 26 |
| trailing data | 14 | 0 | 0 | 14 | 0 | 0 | 0 | 0 |
| encoding | 67 | 0 | 55 | 12 | 0 | 0 | 52 | 52 |
| not an object | 16 | 0 | 0 | 2 | 14 | 0 | 0 | 0 |

Export: equal to the committed file: true; 4 object subschemas, 0 without additionalProperties false, 0 findings. In output mode the export of z.object at cap has 0 findings at cap.
typify: 400 generated lines; structs Request, RequestCap, RequestCapPeriod, RequestDevice; enums RequestCapCurrency, RequestCapPeriodUnit, RequestDevicePlatform, RequestLocale, RequestTier; tuple structs RequestDeviceInstallId, ConversionError; aliases 0. The jsonschema crate on the same export: 2 of 2 unmodified bodies valid, 96 of 96 unknown keys refused. serde_json wrote 2 unmodified bodies back from the generated types: 2 texts equal JSON.stringify of their parsed value and 2 hold the value they were read from.

### allof

Unmodified bodies: 2 of 2 accepted, 2 rows in state. Arbitrary names: 1000 sampled, 898 distinct and sent, the longest 23 UTF-16 units. After all cases state holds 2 rows; the dump read 21 instances of the Durable Object. The Worker wrote 18211 console lines, 4 distinct.

| unknown key, one per body | bodies at root / cap / cap.period / device | accepted | rejected by the validator | rejected by the router rule | not JSON | other |
|---|---|---|---|---|---|---|
| forbidden names | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 32 / 32 / 32 / 32 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| Object.prototype names | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 52 / 52 / 52 / 52 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| __proto__, constructor, prototype with an object value | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 12 / 12 / 12 / 12 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of declared keys | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 348 / 220 / 136 / 152 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| variants of forbidden names | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 608 / 608 / 608 / 608 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |
| arbitrary names | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 3592 / 3592 / 3592 / 3592 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |

| router rule and body shape | bodies | accepted | rejected by the router rule | not JSON | rejected by the validator | other | parsed value valid | accepted with the rule off (K3) |
|---|---|---|---|---|---|---|---|---|
| repeat, sentinel first | 23 | 0 | 23 | 0 | 0 | 0 | 23 | 23 |
| repeat, sentinel last | 23 | 0 | 23 | 0 | 0 | 0 | 0 | 0 |
| padding | 26 | 0 | 26 | 0 | 0 | 0 | 26 | 26 |
| trailing data | 14 | 0 | 0 | 14 | 0 | 0 | 0 | 0 |
| encoding | 67 | 0 | 55 | 12 | 0 | 0 | 52 | 52 |
| not an object | 16 | 0 | 0 | 2 | 14 | 0 | 0 | 0 |

Export: equal to the committed file: true; 5 object subschemas, 0 without additionalProperties false, 1 findings (/properties/device: allOf, which the contract forbids). In output mode the export of z.object at cap has 0 findings at cap.
typify: 297 generated lines; structs Request, RequestCap, RequestCapPeriod; enums RequestCapCurrency, RequestCapPeriodUnit, RequestDevice, RequestLocale, RequestTier; tuple structs ConversionError; aliases 0. The jsonschema crate on the same export: 0 of 2 unmodified bodies valid, 96 of 96 unknown keys refused. serde_json wrote 0 unmodified bodies back from the generated types: 0 texts equal JSON.stringify of their parsed value and 0 hold the value they were read from.

### observations outside the rule

A probe with what the unit's schema does not hold: a discriminated union of three strict objects, one of which holds only the tag, a nullable enum and an array of strict objects (buildProbe in src/schema.ts).

- Zod under workerd, called without the Worker: accepted 3 of 3 probe bodies and refused 27 of 27 bodies with an unknown key, at the positions root, choice, items.[].
- Export: equal to the committed file: true; composition keywords in it: anyOf, oneOf; findings of the closed check: 0.
- typify: the crate builds; structs and struct variants Request, RequestChoice::Count, RequestChoice::Unit, RequestItemsItem; of them 0 do not deny unknown fields; 0 flatten fields; 0 map fields; serde_json read 3 of 3 probe bodies and refused 96 of 108 unknown keys; it read an unknown key at: /choice; the jsonschema crate found 3 of 3 bodies valid and refused 108 of 108.

### controls

- K1 extend: caught. z.object at one position: accepted at root / cap / cap.period / device when that position is open: 32 of 32 / 32 of 32 / 32 of 32 / 32 of 32; accepted at the other positions, beyond what the schema under test accepts there: 0.
- K2 extend: caught. z.looseObject at one position: state rows that hold the sentinel: 32 / 32 / 32 / 32.
- K3 extend: caught. router rule off: accepted, of the bodies whose parsed value is valid, 23 of 23 in "repeat, sentinel first", 0 of 0 in "repeat, sentinel last", 26 of 26 in "padding", 0 of 0 in "trailing data", 52 of 52 in "encoding", 0 of 0 in "not an object".
- K4 extend: caught. a handler that logs the body inside an object and returns it: the sentinel in 128 of 128 rejected responses and in 128 console lines.
- K5 extend: caught. the closed check on the export of each K1 and K2 schema: findings at the changed position, strip / loose: 1 / 3, 1 / 3, 1 / 3, 1 / 3.
- K1 intersection: caught. z.object at one position: accepted at root / cap / cap.period / device when that position is open: 32 of 32 / 32 of 32 / 32 of 32 / 32 of 32; accepted at the other positions, beyond what the schema under test accepts there: 0.
- K2 intersection: caught. z.looseObject at one position: state rows that hold the sentinel: 32 / 32 / 32 / 32.
- K3 intersection: caught. router rule off: accepted, of the bodies whose parsed value is valid, 23 of 23 in "repeat, sentinel first", 0 of 0 in "repeat, sentinel last", 26 of 26 in "padding", 0 of 0 in "trailing data", 52 of 52 in "encoding", 0 of 0 in "not an object".
- K4 intersection: caught. a handler that logs the body inside an object and returns it: the sentinel in 128 of 128 rejected responses and in 128 console lines.
- K5 intersection: caught. the closed check on the export of each K1 and K2 schema: findings at the changed position, strip / loose: 1 / 3, 1 / 3, 1 / 3, 1 / 1.
- K1 allof: caught. z.object at one position: accepted at root / cap / cap.period / device when that position is open: 32 of 32 / 32 of 32 / 32 of 32 / 32 of 32; accepted at the other positions, beyond what the schema under test accepts there: 0.
- K2 allof: caught. z.looseObject at one position: state rows that hold the sentinel: 32 / 32 / 32 / 32.
- K3 allof: caught. router rule off: accepted, of the bodies whose parsed value is valid, 23 of 23 in "repeat, sentinel first", 0 of 0 in "repeat, sentinel last", 26 of 26 in "padding", 0 of 0 in "trailing data", 52 of 52 in "encoding", 0 of 0 in "not an object".
- K4 allof: caught. a handler that logs the body inside an object and returns it: the sentinel in 128 of 128 rejected responses and in 128 console lines.
- K5 allof: caught. the closed check on the export of each K1 and K2 schema: findings at the changed position, strip / loose: 1 / 3, 1 / 3, 1 / 3, 3 / 7.
- K6 open object: caught. typify on an export with z.object at cap: structs that do not deny unknown fields: RequestCap; serde_json read 24 of 96 unknown keys.
- K6 map: caught. typify on a schema-valued additionalProperties: map fields: Request.labels: HashMap; serde_json read 3 of 24 unknown keys.

## How the second result was measured, and what limits it

- **The same measurement.** The bodies, the Worker, the schema, the committed exports and the
  Rust checker are those of the first result. "How each value was measured, and what limits it"
  above holds for these values too.
- **What changed in the code.** The closed check (`src/closed.ts`) reports every `allOf`, and an
  object must carry `additionalProperties: false` itself. The test gives the check two more
  small schemas: an `allOf` of two strings, and an object closed by `unevaluatedProperties`
  alone. The judge counts as the second statement says. No other file that a run reads changed.
- **Flagged.** The judge reads the committed export of `allof` and finds the one schema that
  holds `allOf`, at `/properties/device`. It looks for a finding with the closed check's reason
  for `allOf` at that place in what the check reported under workerd, and asks the same of three
  small schemas. Limits: one export and three schemas. The judge looks for `allOf` in every
  value of the file, so an `enum`, `const` or `default` value with a key of that name would
  count as a schema, and the line would read "not flagged".
- **The allowed forms.** The judge searches the committed export of each allowed form in the
  same way. An `allOf` there turns the validator verdict of the form to MISSED, also where the
  closed check reports nothing. The typify line turns only when the generated types fail.
- **The first statement's reading.** The fifth line counts the three forms as the first
  statement did, on the measurements of this run and with this run's closed check.
- **A run that does not count.** The first line and the exit code say so. The lines below it
  still print the words they would have had.
- **Runs.** `sh spikes/s3/run.sh` ran twice in the worktree at dde000f and once in a fresh clone
  of the branch at that commit, after `sh scripts/prerequisites.sh`. The three runs printed the
  same text and exited 0. Each run in the worktree took 19 s of wall time, read from `date`
  around the run, while the whole-run control ran beside it. The runs of the first result took
  115 to 232 s on a machine that other jobs loaded. The three runs of the first round, at
  f25e0fe, printed the same text as these except for the wording of the line of `allof`.
- **What the MET does not show.** It was not predicted: the second statement was written with
  these values known. The reruns show that the measurement repeats, and the whole-run control
  below shows that the lines turn when the closed check stops reporting `allOf`. The limits of
  the first result stand: one schema, one unknown key per body, no deployed Worker.
- **What the forbidden form no longer decides.** If Zod accepted an unknown key in `allof`,
  both lines would still read MET, because that form decides neither. The count would show on
  the line of `allof` and in its measured values only. A probe of the judge with one accepted
  body in `allof` reads so.
- **A wider rule than objects.** The closed check fails on every `allOf`, not only on one that
  composes objects. Zod 4.6.7 writes `allOf` for a string with two `regex` checks and for an
  intersection of two strings, read from `z.toJSONSchema` under Node. A contract under this rule
  gives a string one pattern.

## The whole-run control under the second statement

`node spikes/s3/mutation-control.mjs` at dde000f makes fifteen changes, one at a time, in a
scratch copy. Eight are those of the first result, read under the second statement. Three are
the ones that the second statement names: a closed check that no longer reports `allOf`, a
closed check that lets an `allOf` with `unevaluatedProperties: false` pass, and an allowed
intersection that keeps `allOf` in its export. Four were added after the review: a closed check
that reports `allOf` at the root of a schema only, one that reports it only where a branch
describes an object, a Worker that rejects every body of the extended form, and a closed check
that lets an object pass that `unevaluatedProperties` alone closes. A run that must not count
is compared by its whole first line. Its output:

```
mutation-control: a copy of dde000f is in .tmp/s3-mutation.yqexwn
as required: the unchanged copy: run.sh exited 0; rule: MET (validator MET, typify MET); validator: extend MET, intersection MET; typify: extend MET, intersection MET; allof: forbidden by the contract, flagged by the walk
as required: one z.strictObject changed to z.object, then exported again: run.sh exited 1; rule: MISSED (validator MISSED, typify MISSED); validator: extend MISSED, intersection MET; typify: extend MISSED, intersection MET; allof: forbidden by the contract, flagged by the walk
as required: the router rule removed: run.sh exited 1; rule: MISSED (validator MISSED, typify MET); validator: extend MISSED, intersection MISSED; typify: extend MET, intersection MET; allof: forbidden by the contract, flagged by the walk
as required: every rejection logs and returns the body: run.sh exited 1; rule: MISSED (validator MISSED, typify MET); validator: extend MISSED, intersection MISSED; typify: extend MET, intersection MET; allof: forbidden by the contract, flagged by the walk
as required: a rejection also logs the body inside an object, the answer stays fixed: run.sh exited 1; rule: MISSED (validator MISSED, typify MET); validator: extend MISSED, intersection MISSED; typify: extend MET, intersection MET; allof: forbidden by the contract, flagged by the walk
as required: a rejection also returns the body in a header, the body of the answer stays fixed: run.sh exited 1; rule: MISSED (validator MISSED, typify MET); validator: extend MISSED, intersection MISSED; typify: extend MET, intersection MET; allof: forbidden by the contract, flagged by the walk
as required: a rejected body is written to another instance of the Durable Object: run.sh exited 1; rule: MISSED (validator MISSED, typify MET); validator: extend MISSED, intersection MISSED; typify: extend MET, intersection MET; allof: forbidden by the contract, flagged by the walk
as required: the committed export opened at cap, the schema unchanged: run.sh exited 1; rule: MISSED (validator MISSED, typify MISSED); validator: extend MISSED, intersection MET; typify: extend MISSED, intersection MET; allof: forbidden by the contract, flagged by the walk
as required: the closed check no longer reports allOf: run.sh exited 1; rule: MISSED (validator MISSED, typify MET); validator: extend MET, intersection MET; typify: extend MET, intersection MET; allof: forbidden by the contract, not flagged
as required: the closed check lets an allOf with unevaluatedProperties false pass: run.sh exited 1; rule: MISSED (validator MISSED, typify MET); validator: extend MET, intersection MET; typify: extend MET, intersection MET; allof: forbidden by the contract, not flagged
as required: the allowed intersection keeps allOf in its export, then exported again: run.sh exited 1; rule: MISSED (validator MISSED, typify MISSED); validator: extend MET, intersection MISSED; typify: extend MET, intersection MISSED; allof: forbidden by the contract, flagged by the walk
as required: the closed check reports allOf at the root of a schema only: run.sh exited 1; rule: MISSED (validator MISSED, typify MET); validator: extend MET, intersection MET; typify: extend MET, intersection MET; allof: forbidden by the contract, not flagged
as required: the closed check reports allOf only where a branch describes an object: run.sh exited 1; rule: MISSED (validator MISSED, typify MET); validator: extend MET, intersection MET; typify: extend MET, intersection MET; allof: forbidden by the contract, not flagged
as required: the Worker rejects every body of the form extend: run.sh exited 1; rule: MISSED (validator MISSED, typify MISSED); validator: extend MISSED, intersection MET; typify: extend MISSED, intersection MET; allof: forbidden by the contract, flagged by the walk
as required: the closed check lets an object pass that unevaluatedProperties alone closes: run.sh exited 2 and printed "rule: not decided. The run does not count: the closed check does not flag its own fixtures."
as required: no arbitrary name is sampled: run.sh exited 2 and printed "rule: not decided. The run does not count: extend: no body of "arbitrary names" at root; extend: no body of "arbitrary names" at cap; extend: no body "
```

Each of the four changes to how the closed check reports `allOf` turns the validator line to
MISSED and leaves both allowed forms MET. Before the judge was changed, the first of them made
the run not count: the judge of 7d0c260 exited 2 on it, and its validator line read MET.

## Reproduce both results

```sh
sh scripts/prerequisites.sh           # once; the only step that uses the network
sh spikes/s3/run.sh                   # the second result: five rule lines, then the measured values
node spikes/s3/mutation-control.mjs   # the whole-run control under the second statement
```

`sh spikes/s3/run.sh` prints the rule line, the validator line and the typify line as its first
three lines. It exits 0 when both lines read MET, as it does here, 1 when one reads MISSED and 2
when the run does not count.

The first result is reproduced by the same command in a checkout of commit 7d0c260, where it
exits 1. At this commit the fifth line of the output gives the same reading: `rule: MISSED
(validator MISSED, typify MISSED)`.
