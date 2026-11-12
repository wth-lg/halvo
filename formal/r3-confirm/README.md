# The model of the confirm policy

This directory holds the model of the confirm policy of the agent run (items A5.1 to A5.6
of that run's contracts): the table that decides whether an action runs, waits for the
person, is handed back or is refused, and the state machine of one effect. A model checker
searches every order of moves of a small system that is built from both. Two files come
from the model, and the code is compared with them: `policy-table.tsv` with `decide_row`,
row by row, and `traces.jsonl` with `effect_step`, move by move.

| File | What it holds |
|---|---|
| `Confirm.tla` | The model: the decision `Decide`, the step relation `Step`, the system with the invariants P1 to P11, and the operators that print the two exported files. |
| `Confirm.cfg` | The first of the two configurations that the check runs: two effects, two approvals, two digests, one rebind for each effect. |
| `Rebinds.cfg` | The second: one effect, two approvals, two digests, two rebinds. |
| `Full.cfg` | Two effects and two rebinds. The check does not run it; a person does, after each change of the model. |
| `Control.cfg` | The control copy: `Confirm.cfg` with the switches `PaymentMayRun` and `BeginUnguarded` on. TLC must report P1. |
| `Export.cfg` | The export, with the seed and the count of the committed traces. |
| `export.sh` | Writes the two files below. A person runs it. |
| `policy-table.tsv` | The 3,840 rows of `Decide`. |
| `traces.jsonl` | 400 traces of `Step`. |

The two exported files are never edited by hand. A change of the table or of the machine is
made in `Confirm.tla`, checked, and exported again.

## The checker

The checker is TLC, the model checker of the TLA+ tools. It is one file, `tla2tools.jar`,
taken from the tlaplus project's own release. This line names the release, the address,
the size in bytes and the SHA-256 of the file:

```
tla2tools.jar release=v1.7.4 url=https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar bytes=2274532 sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
```

It is the release and the hash that the backend repository pins in
`scripts/fetch-tools.sh` and `scripts/tools.sha256`, so both repositories check their
models with one file.

The jar is a check tool. It reads the model and prints what it finds; it is used to check
the model and to export the two committed files, and for nothing else. Nothing of it is in
this repository: no file of the jar is committed, no code links it and no build ships it.
A command that needs it takes a copy that lies outside the work tree and compares its
SHA-256 with the line above before the first use. The archive carries two licence files:
`License.txt` holds the MIT licence, and `META-INF/LICENSE.txt` holds the Eclipse Public
License 2.0 and, below it, the GNU General Public License version 2.

TLC runs on a Java of major version 17 or later.

## Commands

TLC writes its state files into the directory that `-metadir` names, and it unpacks its
own modules into Java's temporary directory. Run it on a copy of the model in a scratch
directory, and give each run that directory as `java.io.tmpdir`, so that this directory
gains no file and two runs on one machine do not write the same files. `JAR` is the path
of the jar.

```sh
scratch=$(mktemp -d)
cp formal/r3-confirm/Confirm.tla formal/r3-confirm/*.cfg "$scratch"
cd "$scratch"
java -XX:+UseParallelGC -Xmx2g -Djava.io.tmpdir="$scratch" -cp "$JAR" tlc2.TLC -workers 1 -deadlock -metadir "$scratch/one" -config Confirm.cfg Confirm.tla
java -XX:+UseParallelGC -Xmx2g -Djava.io.tmpdir="$scratch" -cp "$JAR" tlc2.TLC -workers 1 -deadlock -metadir "$scratch/two" -config Rebinds.cfg Confirm.tla
java -XX:+UseParallelGC -Xmx2g -Djava.io.tmpdir="$scratch" -cp "$JAR" tlc2.TLC -workers 1 -deadlock -metadir "$scratch/control" -config Control.cfg Confirm.tla
```

The first two commands are the check, and both are needed: neither alone shows every
invariant failing on its break. Each ends with status 0 and prints `Model checking
completed. No error has been found.`; the first prints `637933 distinct states found`, the
second `61728 distinct states found`. The third is the control copy. It ends with status 12
and prints `Error: Invariant P1 is violated.` with a behaviour of four states. A check that
passes proves nothing until the control copy fails. The option `-deadlock` turns off TLC's
search for states with no next state: an effect has final states.

`Full.cfg` is run by hand, with the same command and `-workers 4`. It prints `3570126
states generated, 1500352 distinct states found`. On 2026-11-11 it took 128 seconds with
four workers on a 16-core machine whose load average was 200. After the round of CP-35, on
2026-11-12, it printed the same counts and took 306 seconds with four workers through the
machine's timed-run helper, at a 1-minute load average of 85 before and 94 after. Run it
after every change of `Confirm.tla`, and run every wrong design of the table that belongs to
the model with it.

```sh
TLA2TOOLS_JAR="$JAR" sh formal/r3-confirm/export.sh
TLA2TOOLS_JAR="$JAR" sh formal/r3-confirm/export.sh --out DIR
```

The first form writes the two files into this directory, the second into `DIR`. With the
seed and the count of `Export.cfg`, or with the same `--seed` and `--count`, a second run
writes the same bytes. The script prints one line, `export rows=3840 traces=400
directed=304 seed=20261113`; `directed` is a key of that line and no key of the file.
`sh formal/r3-confirm/export.sh --help` prints the options, the exit statuses and the three
causes of a refusal. The script runs TLC in a directory from `mktemp -d` and leaves that
directory in place.

## The decision

`Decide(aut, cls, allow, caps, ck)` is `decide_row(autonomy, class, allow, caps,
checkpoint)` of A5.1, the function of the table's rows: seven conditions, and the first
that fits gives the decision. One line of `policy-table.tsv` is one call. Its six columns,
with a tab between them:

| Column | Argument | Values |
|---|---|---|
| 1 | `autonomy` | `observe`, `draft_and_confirm`, `unattended` |
| 2 | `class` | the ten names of `ActionClass` |
| 3 | `allow` | the allowed classes in byte order with a comma between them, of `delete`, `send`, `submit`; `-` for the empty set |
| 4 | `caps` | three letters for `untrusted`, `private` and `external` in this order; `y` is `yes`, `n` is `no` |
| 5 | `checkpoint` | `yes`, `no` |
| 6 | the decision | `run`, `confirm`, `handback`, `refuse` |

The lines are sorted in byte order. There are 3 x 10 x 8 x 8 x 2 = 3,840 of them.

## The step relation

`Step(x, m)` is `effect_step(effect, move)` of A5.2. `x` is the effect as it stands: its
id, its state, its class, its digest and the count of its `rebind` rows. `m` is one move.
The answer is `ok` with the next state, or a cause with the state unchanged. The conditions
of a move are read from top to bottom, and the first that fits gives the answer.

| Move | Conditions, in this order | Answer |
|---|---|---|
| `propose` with a decision | an effect exists | `effect` |
| | the class is `read`, `prepare` or `credential` | `effect` |
| | the decision is `refuse` | `ok`, `refused` |
| | the decision is `run` | `ok`, `cleared` |
| | the decision is `confirm` | `ok`, `held` |
| | the decision is `handback` | `effect` |
| `rebind` with a digest and the class of the new bound object | the state is not `held` and not `approved` | `effect` |
| | the class is not the effect's | `class` |
| | the digest is the effect's | `effect` |
| | the effect has two `rebind` rows | `changed` |
| | else | `ok`, `held`; the effect's digest is now the move's, and its count is one more |
| `approve` with the effect id of its entry, the entry's digest and `used` | the state is not `held` | `effect` |
| | the entry was given for another effect | `approval` |
| | the digest is not the effect's | `digest` |
| | `used` is `yes` | `used` |
| | else | `ok`, `approved` |
| `reject` | the state is not `held` | `effect` |
| | else | `ok`, `rejected` |
| `end` | the state is not `held`, `cleared` or `approved` | `effect` |
| | else | `ok`, `rejected` |
| `begin` with the digest that is read again | there is no effect, or the state is `refused`, `rejected`, `committed` or `releasing` | `effect` |
| | the state is `unknown` | `unknown` |
| | the state is `held` | `approval` |
| | the state is `cleared` and the class is `purchase`, `payment` or `sign` | `approval` |
| | the digest is not the effect's | `digest` |
| | else | `ok`, `releasing` |
| `commit` | the state is not `releasing` | `effect` |
| | else | `ok`, `committed` |
| `lose` | the state is not `releasing` | `effect` |
| | else | `ok`, `unknown` |

The decision of a `propose` is a free value here: the step relation makes a `cleared`
effect of the class `payment` when it is told `run`, and then refuses its `begin`. In the
code `effect_step` takes the decision from a `Decided`, and the test `formal_traces` makes
a free one with `Decided::free`. The limit of two rebinds is a value of the step relation
itself (`Limit`), whatever bound a configuration sets for the search.

## The traces

`traces.jsonl` holds one JSON object per line, compact, with its keys in the order shown.
The first line is `{"seed":<n>,"count":<n>}`. Each of the `count` lines after it is one
trace: the life of one effect, from no effect, in at most 12 moves.

```
{"n":<n>,"effect":"e1","bound":"<digest>","moves":[{"class":"<class>","move":"<move>","decision":<decision>,"entry":<effect id>,"digest":<digest>,"bound_class":<class>,"used":<used>,"answer":"<answer>","state":<state>,"bound":"<digest>"},...]}
```

| Key | Meaning |
|---|---|
| `n` | The number of the trace, from 1. |
| `effect` | The id of the effect of the trace. It is `e1` in every trace. |
| `bound` of the trace | The digest of the effect before the first move. |
| `class` | The class of the effect. It is the same in every move of a trace. |
| `move` | `propose`, `rebind`, `approve`, `reject`, `end`, `begin`, `commit` or `lose`. |
| `decision` | For `propose`: `run`, `confirm`, `handback` or `refuse`. Else `null`. |
| `entry` | For `approve`: the effect id of the entry, `e1` for this effect or `e2` for another. Else `null`. |
| `digest` | For `rebind` the new digest, for `approve` the digest of the entry, for `begin` the digest that is read again. Else `null`. |
| `bound_class` | For `rebind`: the class of the new bound object. Else `null`. |
| `used` | For `approve`: `yes` or `no`. Else `null`. |
| `answer` | `ok`, or the cause: `effect`, `class`, `changed`, `approval`, `digest`, `used`, `unknown`. |
| `state` | The state of the effect after the move, `null` while there is no effect. A refused move leaves it as it was. |
| `bound` of a move | The digest of the effect after the move: the move's digest after a `rebind` with the answer `ok`, else as before. |

`d1` and `d2` stand for two digests that differ, and `e1` and `e2` for two effect ids that
differ. A test maps them to any two values of the code's types.

To replay a trace with `effect_step`, keep a state, a digest and a count. Start with no
state, the `bound` of the trace and the count 0. For each move, call `effect_step` with the
effect as it stands (the trace's `effect`, the kept state, the move's `class`, the kept
digest, the kept count) and with the move built from `move`, `decision`, `entry`, `digest`,
`bound_class` and `used`. The call must accept exactly when `answer` is `ok`, and the next
state must be `state`. Then keep `state` and the move's `bound`, and add one to the count
when the move is a `rebind` with the answer `ok`. No key holds the count: it is the number
of the trace's earlier accepted rebinds. The test `formal_traces` compares no causes
(A5.6); the file holds the model's cause.

The first 304 traces are directed and do not depend on the seed. For each of the seven
classes that can make an effect there are eleven places: no effect, the eight states, and
`held` and `approved` after two accepted rebinds. A fixed path leads to each place; the
path to the last one has its first rebind from `approved` and its second from `held`. Then
the 22 moves are tried in a fixed order: `propose` with each of the four decisions;
`rebind` with each digest and with the effect's class or another; `approve` with each
entry, each digest and each `used`; `reject`; `end`; `begin` with each digest; `commit`;
`lose`. For `read`, `prepare` and `credential` no place but the first can be reached, and
the 22 moves are tried there. A trace ends at its twelfth move or at a move that is
accepted, and the next trace goes on with the next move. So each of the 1,760 triples of
class, place and move that can be met is in the file: 7 x 11 x 22 and 3 x 1 x 22. Fourteen
of them are the `rebind` that is refused with `changed`. The other 96 traces are drawn
from the seed: a class, a first digest, 4 to 12 moves, each move with 60 chances in 100
one that the state accepts when there is one, else any of the 22. The export fails when
`--count` is below 304.

## The system and the invariants

The invariants are checked on a system of effects, not on the step relation alone. Its
variables:

| Variable | What it is | Who writes it |
|---|---|---|
| `cfg` | The configuration: autonomy, `allow`, `caps`, and whether the parser's guard let it through. | The start state chooses any of 3 x 1,024 x 8 raw configurations, with every set of the ten classes as `allow`. `Parse` lets one through when `allow` holds only `send`, `delete` and `submit`. |
| `eff` | The rows of each effect, as far as a move reads them: the state, class and digest of the last row, the approval that it holds, the count of its `rebind` rows, and every approval that a row of the effect ever held. | The moves of the runner and of the handler, each through `Step`. |
| `mem` | What the living runner knows of its request: nothing, `begun` (the `begin` is stored and the request is paused), `sent`, `replied`. | `Begin`, `Send`, `Reply`, `Commit`. `RunEnds` empties it. |
| `alive` | Whether the run goes on. | `RunEnds`. |
| `val` | The digest of the bound values of each effect as they are on the page, in the call or in the file. | The environment: `Propose` and `Change`. |
| `given` | The entries of the supervisor's decisions: the effect that was shown, the digest that was shown, and the answer `allow` or `reject`. | The environment: `Give`. |
| `wire` | The requests that left: the digest that each carried, the state of the row when it left, whether the run went on when it left, the arrivals in `releasing` that were stored before, the digest that the person was shown last, and whether the reply came. | The environment: `Send` and `Reply`. |
| `seen` | What was observed of each effect: its class, each decision that `Decide` gave for it, each arrival of its row in `approved` in order (the entry that the move offered, its answer, the effect that it was given for, the digest that was shown, the digest of the row), each arrival in `releasing` with the state before and the row's digest, each final state that a move left, and for each arrival in `committed` whether the request had left and the reply had come. | Every move that changes a row, from the row before and the row after the move. |

No invariant reads the answer of `Step` or a claim of a move about itself. Each reads the
histories `seen` and `wire`, which the moves write from `given`, `val` and the rows. P5 and
P7 also read the configuration, and P11 the stored row.

| | The sentence of A5.3 | What the invariant reads |
|---|---|---|
| P1 | An effect of the class `purchase`, `payment` or `sign` reaches `releasing` only from `approved`. | Each arrival of such an effect's row in `releasing` came from `approved`, and each request of it left after such an arrival was stored. |
| P2 | An effect reaches `approved` only with an approval that was given for that effect, with the answer `allow_once`, and whose digest equals the digest of the effect's row, which is what the runner sent. | Each arrival of a row in `approved` came with an entry of the supervisor that was given for this effect, whose answer is `allow`, and whose shown digest is the digest of the row. |
| P3 | An approval moves an effect to `approved` at most once. | Over all effects, the arrivals in `approved` with one entry are at most one. |
| P4 | No move leaves `refused`, `rejected`, `committed` or `unknown`, no effect is in `releasing` twice, and at most one request leaves per effect, where a repeat that is answered as the first while its row is the last is no move. | No row left a final state; no row arrived in `releasing` twice; no effect's request left twice. The model has no repeat. |
| P5 | Under the autonomy `observe` no effect reaches `releasing`. | Under `observe` no row arrived in `releasing` and no request left. |
| P6 | An effect whose decision is `handback` or `refuse` never reaches `releasing`. | The same, for an effect for which `Decide` ever gave one of the two. |
| P7 | `allow` never holds a money class, `sign` or `credential`, in any reachable configuration. | A configuration that the guard let through, or under which an effect exists, has none of `purchase`, `payment`, `sign`, `credential` in `allow`. |
| P8 | An effect for which `decide` gave `confirm` reaches `releasing` only from `approved`, and its request leaves only after that. | As P1, for every effect for which `Decide` gave `confirm`. |
| P9 | An effect reaches `committed` only after its request left and the reply to that request came. | At each arrival of a row in `committed`, the wire held a request of the effect with its reply. |
| P10 | Every request that leaves carries the digest of its effect's row at `begin`, and for an effect for which `decide` gave `confirm` that is the digest that the person was shown for the approval that moved it to `approved` last. | Each request left while its row was `releasing` and while the run went on, after a stored `begin`, with the digest that the row had at that `begin`; for an effect for which `Decide` gave `confirm` it is also the shown digest of the last arrival in `approved`. |
| P11 | An effect whose row once was `releasing` is `releasing`, `committed` or `unknown`, and in no other state. | An effect with an arrival in `releasing` has a row in one of the three states. It reads the rows and not the wire, so it also covers an end of the run between the stored `begin` and the send. |

The moves of the system, and what each takes from the contract:

- `Propose` (runner) makes the row of an effect with the decision that `Decide` gives for
  the configuration, the class and the checkpoint. `Step` refuses it for `read`, `prepare`
  and `credential`.
- `Change` (environment) is a bound value that changes while the effect is `held`,
  `approved` or `cleared`. Only `val` changes. No value changes while an effect is
  `releasing`: that is an assumption of the model, and the paused request of A11.1 in the
  code.
- `Rebind` (runner) reads the value again and passes it with the effect's class. `Step`
  accepts it for `held` and `approved` below the limit; the row is then `held` and holds no
  approval. The constant `MaxRebinds` of a configuration only bounds the search: it is 1 in
  `Confirm.cfg` and 2, the limit itself, in the other two.
- `Give` (environment) is one entry of the supervisor for a `held` effect, with the digest
  of the row, which is what the runner showed. `Approve` (runner) offers an entry whose
  answer is `allow`, with the entry's effect and digest. `used` is `yes` when a row of any
  effect ever held the approval, and it is read in the step that stores the row.
- `Begin` (runner) passes the digest of the values as they are, read again, and the row is
  `releasing`. `Send` (runner) then lets the request leave, once, and the wire notes whether
  the run went on when it left. `Reply` (environment) is
  the reply to it. `Commit` (runner) stores the end after the reply.
- `Reject` is the runner's. `End` may come at any time, from the runner or from the handler
  of the run's end.
- `RunEnds` is the end of the run, at most once, in any state. It stands for the runner's
  `finish`, before which the runner fails every request that it holds paused, and for the
  death of the runner, which is its process being gone by death or by reaping. The model
  does not tell the two apart: after either, no move of the runner comes, none of the
  environment, and no request leaves.
- `Lose` is the move of the handler of the run's end and comes only after `RunEnds`. So no
  request leaves after its row became `unknown`.

Four rules of the step relation are held by the traces alone, because the system never
makes the move that would break them:

- a `rebind` with another class than the effect's: the system passes the effect's own
  class;
- a `rebind` at the limit: the system's `Rebind` stops at `MaxRebinds`, which is at most
  the limit;
- a `begin` for an `unknown` effect: `lose` comes after the run's end, and no `begin` comes
  after it;
- a `propose` with the decision `handback` for a class that can make an effect: `Decide`
  gives `handback` for `credential` alone.

## What the check reaches

The check runs two configurations of one model text, and a person runs a third:

| File | Effects | Rebinds for each effect | Distinct states | Depth |
|---|---|---|---|---|
| `Confirm.cfg` | 2 | 1 | 637,933 | 21 |
| `Rebinds.cfg` | 1 | 2 | 61,728 | 16 |
| `Full.cfg` | 2 | 2 | 1,500,352 | 25 |

Each has two approvals and two digests, and the line `SYMMETRY` counts states that differ
only in the names of the effects, the approvals and the digests once. On 2026-11-11, on a
16-core machine whose load average was between 129 and 207, `Confirm.cfg` took 204 seconds
with one worker and 54 seconds with four, and `Rebinds.cfg` 13 seconds with one worker and
5 with four.

What the split into the first two loses against the third: the runs in which one effect is
rebound twice while a second effect exists. A wrong design that needs two rebinds (an
approval that moves its effect again after a rebind away and back) needs one effect only
and is reported under `Rebinds.cfg`; one that needs two effects (an entry that was given
for the other effect) needs no rebind and is reported under `Confirm.cfg`.

Runs are searched for one configuration of each set of configurations that `Decide` cannot
tell apart, and for one class of each set of classes that the configuration and `Step`
cannot tell apart:

- All 24,576 raw configurations meet the guard, and P7 is checked on each of the 192 that
  it lets through. An effect is proposed, and a run ends, only under a configuration in
  normal form (`Norm`): `allow` is empty and `caps` are all `yes` but `external`, unless
  the autonomy is `unattended`, `allow` is not empty and `untrusted` or `private` is `no`;
  then `allow` stays and `untrusted` and `private` are `no`. There are 10 normal forms. The
  assumption `NormKeepsDecisions`, which TLC checks before it starts, says that a
  configuration and its normal form have the same autonomy and the same decision for every
  class and checkpoint. After the guard the moves read the configuration through `Table`
  and nothing else, and P5 reads the autonomy.
- An effect is proposed only for the first class of each set of classes that are alike
  (`Lead`): they are of one kind, and the configuration gives them the same decision at
  each checkpoint. There are three kinds (`Kind`): `purchase`, `payment` and `sign`, which
  need an approval at `begin`; `read`, `prepare` and `credential`, for which `Step` refuses
  `propose`; and the other four. The assumption `KindIsEnough` says that `Step` answers
  every move for a class as it answers it for the class's kind, in every state and at
  every count.

A later change of the model has to keep both assumptions true. When `Step` reads the class
in a new place, `Kind` needs a value for it, or TLC stops before it searches; when a move
reads the configuration in a new place, `Norm` has to keep what that move reads, and TLC
does not check that by itself.

So a trace of TLC shows `purchase` where the same holds for `payment` and `sign`, and
`send` where the same holds for a class that the configuration treats as it treats `send`.

## When the policy changes

The model describes four things of the code: `decide_row`, `effect_step`, the parser's type
`AllowedClass`, and the rules of the caller that the moves above name (the decision of a
`propose` is the one that `decide` gives; `begin` carries the digest that is read again and
is stored before the request is let through; one request leaves per effect, and none after
the run's end; `commit` is stored after the reply; `used` is read in the step that stores
the approve; `lose` comes at the run's end and at no other time; no row is ever removed). A
change of one of them, or of A5.1 to A5.3, is a change of the model. The model was written
before the code. The crate halvo-policy compares the code with the two files on every push:
its test `policy_table` compares `decide_row` with each row of `policy-table.tsv`, and its
test `formal_traces` follows each trace of `traces.jsonl` with `effect_step`, which must
accept and refuse each move as the model does and reach the same next state, with each of
three ways of giving `d1`, `d2`, `e1` and `e2` values of the code's types.

1. Change `Confirm.tla` first. Run the check and the control copy, and `Full.cfg` by hand.
2. Run `export.sh` and commit the model and the two files together. The tests that read
   the two files then show where the code differs.
3. When TLC reports an invariant, read its behaviour: each state names the move that made
   it. Follow the moves in the code. If the code can make every one of them, the code
   breaks the property, and that is a fault of the code. A move that the code cannot make
   marks a fault of the model: change the model, and leave the invariant as it is.
4. When a row of `policy-table.tsv` and `decide_row` differ, A5.1 says which is right.
5. A new move that changes a row has to call `Noted`, or no invariant sees what it did.

## What the model does not cover

- `class_of`, `digest`, `admit`, `validate` and `preset`. The class of a step and the digest
  of a bound object are given to the model. That `admit` passes before every `propose` and
  `rebind` is the runner's.
- The frames, the dialog, the store's requests and the executors. An entry of the
  supervisor appears; a request leaves; its reply comes or does not.
- A repeated request. The handler's rule, the first answer while the row is the last, is
  in the model for no move; a test of the unit that builds the handler holds it for every
  move.
- The two ways in which a run ends. The model has one move for both, and it takes from the
  code that no request of a run leaves after the run's end. The runner fails its paused
  requests before `finish`; what keeps a request from leaving after the runner's process is
  gone is the code's, and A5.3 names its keeper. A model text in which such a request
  leaves for a row that is not `releasing` breaks P10. One in which it leaves after the end
  while its row is still `releasing`, before the handler's `lose`, breaks P10 as well: P10
  holds that a request leaves only while its row is `releasing` and while the run goes on
  (the coordinator's ruling CP-35). The record of a run, the body `end_run` and the cause
  `ended` of a row are the store's.
- A request that the browser sends while the person holds it in a handback of the reasons
  `sign_in`, `second_factor` or `captcha`, until the `bind_reply` that ends it. Such a
  request is no effect of the run (A5.2): it has no row, and it is not on the wire that the
  model holds.
- More than two effects, two approvals, two digests or one end of the run; two rebinds of
  one effect beside a second effect, outside `Full.cfg`.
- That anything ever happens. Every invariant says what never happens; none says that an
  effect reaches an end.
- A page that sends other values than it shows. P10 holds for the values that the runner
  reads again; the model's `val` is those values.
- Windows. The export was run on macOS only. The model computes the traces with whole
  numbers below 2^31 and prints them in the order of their numbers; `export.sh` takes
  carriage returns out of TLC's output and sorts in byte order.
