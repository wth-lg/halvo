# Case files of the learning run

This file is for the author of the held-out cases (U0.5) and for the
builder of the second case reader in the gate host (U2.2). It repeats
items L9.1 to L9.4 of the run's contracts.md and the line of L7.4 with
its two author calls. It then states how the reader in
`tools/r5-trials/drift` reads the sentences that the contract leaves
open, and it ends with a worked example that the reader accepts. Where
this file and contracts.md differ, contracts.md stands.

## The file (L9.1)

A case is one file `<name>.jsonl`, in the repository under
`fixtures/r5/cases/dev/` or `fixtures/r5/cases/heldout/`; `<name>` is 1
to 32 bytes of `a` to `z`, `0` to `9` and `-`. Line 1 is the header,
every later line one occurrence.

```
{"case":"<name>","kind":"benign"|"fault","form":"<form>","shows":0|1|2,"steps":[{"n":<count>,"verb":"click"|"fill"|"extract","control":"<control id>"}...]}
{"day":<day>,"page":[{"id":"<control id>","role":"<role>","name":"<text>","test_id":"<text>","passes":[<count>...]}...],"repair":{"<n>":["<control id>"...]},"flaky":[<count>...],"restart":"yes"|"no"}
```

## Meaning (L9.2)

`steps` is the task: 1 to 8 steps numbered from 1 with no gap, each
aimed at one control of the page, its intended control, named by
`control`. A `<control id>` is 1 to 16 bytes of `a` to `z`, `0` to `9`
and `-`; it is the control's identity for scoring, and no locator may
read it. `day` is a whole number from 1 to 4,294,967,295, the scripted
day of the occurrence, and rises strictly from line to line. `page` is
the list of controls on that day, at most 16, each `id` once, with the
intended control of every step among them; `role` is one of `button`,
`link`, `textbox`, `checkbox`, `combobox` and `text`; `name` and
`test_id` are 1 to 64 bytes of printable ASCII and hold no personal
value; `passes` may be empty.

The compiled playbook of a case is made from the page of the first
occurrence: step n gets two locators, `test_id` with the intended
control's test id, then `role` with its role and name. A locator
resolves on a page when exactly one control matches it; a step's
locators are tried in order and the step acts on the control of the
first that resolves. A step's condition holds when it acted on its
intended control, or on a control whose `passes` list holds the step's
number: such a control is a decoy that the step's own check cannot tell
from the right one. A step fails with the cause `step_failed` when no
locator resolves or when its condition does not hold after it acted.

`repair` holds, for every step and written under its number as a
string, the one or two controls that the stub repairer names when that
step fails, in order; the last of them is always the step's intended
control. The repairer is asked when a step fails with `step_failed`,
and never at a `timeout`: the runner acts on the first named control,
then on the second when the condition did not hold; when the condition
holds the step is recovered, and the proposal is the served step with
the locator `role` of that control's role and name put first and with
that control's description. Because the last named control is the
intended one, every occurrence of a case that the reader accepts
completes, in every arm. `flaky` lists steps that fail once on that day
with the cause `timeout`, before any locator is tried; the step is then
tried again and behaves as the page says. `restart` `yes` makes the
gate host close and open the host and the store before the occurrence;
the trial's harness, which has neither, reads the key and does nothing.
A case has 8 to 16 occurrences and spans at least `calendar_days_min`
days (240) from its first to its last, and at least one occurrence more
than 93 days after the first holds `restart` `yes`.

## The forms (L9.3)

`shows` is 0 in a benign case and 1 or 2 in a fault case. A change of a
control gives it a new test id and a new name on the same day. Each case
has exactly one form. A reader checks exactly the rules that the bullets
and the paragraph under them state, and no other property of a form;
the page changes only as the form's bullet says, and `flaky` is empty
outside the form `flaky`.

- `renamed` (benign): the intended control of one step changes once, at
  the third occurrence or later, and at least three occurrences follow.
- `renamed-twice` (benign): as `renamed`, and the same control changes a
  second time, at least three occurrences after the first change.
- `alternates` (benign): the change day is as in `renamed`, with at
  least four occurrences after it; from it on the page shows the new and
  the old form of the control in turn, the new one first.
- `flaky` (benign): as `renamed`, and the changed step is flaky on the
  occurrence after the change.
- `decoy` (fault): the change day is as in `renamed`, with at least
  `shows` + 2 occurrences after it. From the change day on the page also
  holds a decoy, a control of the same role that `passes` the changed
  step on that day and on every occurrence before the one named next,
  and `repair` names the decoy first on the change day. On the
  occurrence that is `shows` occurrences after the change, and on every
  later one, the decoy passes nothing and stays on the page. On a still
  later occurrence, the recurrence, the intended control changes again
  while the decoy keeps its name.
- `reused` (fault): the change day is as in `renamed`. On the
  occurrence that is `shows` occurrences after it, the intended control
  changes again, and its name of the change day now stands on another
  control of the same role, which passes nothing. On a still later
  occurrence, the recurrence, the intended control changes a third time
  while that other control keeps the name.

In a benign case `plain` completes with `harm` 0 and `assisted` equal to
`drifts`, and `none` has `assisted` of at least `drifts` + 2. In a fault
case `harm` of `plain` is at least 3 for `decoy` and at least 2 for
`reused`. In a fault case `repair` names the step's intended control
alone on every day but the change day of `decoy`. No control but a
step's intended one carries a test id, or a role with a name, that the
intended control has or had; the one exception is the other control of
`reused`, which carries the name of the change day and a test id of its
own. The decoy's name and test id are new on the change day. The reader
computes these with its own arms `none` and `plain` and refuses a case
that misses them.

## The refusals (L9.4)

Each refusal is one line `REFUSED case: <file name>: <rule>` on standard
error, with status 2 and nothing on standard output: a line that is not
the shape of L9.1; a header `case` that is not the file's name without
`.jsonl`; a count or a form of L9.2 outside its limits; an occurrence
whose `repair` lacks a step or names a control that the page lacks, or
whose last named control is not the intended one; a first page on which
a compiled locator does not resolve to its intended control; a case
with no `restart` `yes` more than 93 days after its first day; a case
that breaks a rule of its form in L9.3. The two readers, U0.3's and
U2.2's, hold the same list: a file that one accepts and the other
refuses is a defect of a reader, and never a reason to change a case.
Scoring is the harness's, from `id` alone.

## The trial's line and the two author calls (L7.4)

`sh tools/r5-trials/drift/run.sh` with no argument prints, for each
case of `fixtures/r5/cases/dev` in byte order of its file name, four
lines, for the arms `none`, `plain`, `admit` with `sort=yes` and `admit`
with `sort=no`, and last the verdict line `verdict rule=<admit|plain>`:

```
case name=<name> kind=<benign|fault> form=<form> arm=<arm> sort=<yes|no|-> occurrences=<n> drifts=<n> assisted=<n> failed=<n> harm=<n> reverted_good=<n>
```

`drifts` is the number of occurrences after the first on which, for at
least one step, the intended control has a test id that it had on no
earlier day and a role with a name that it had together on no earlier
day. `assisted` is the number of occurrences in which the repairer was
asked for at least one step; `failed` the number that did not complete;
`harm` the number of occurrences in which a step acted at least once on
a control that is not its intended control; `reverted_good` the number
of reverts of an edit whose first locator, on the day of the revert,
resolved to the step's intended control or to nothing.

The author of the held-out cases uses two calls:

- `sh tools/r5-trials/drift/run.sh --case FILE --arm none|plain` prints
  the one line of the case file FILE, at any path, for that arm, or
  refuses the file.
- `sh tools/r5-trials/drift/run.sh --count DIR` prints, for the case
  files of DIR, one line
  `spread form=<form> cases=<n> shows1=<n> shows2=<n>` per form, in the
  order `renamed`, `renamed-twice`, `alternates`, `flaky`, `decoy`,
  `reused`.

## How this reader reads the open sentences

1. The shape. A line is accepted only in the one compact form of its
   value: the keys in the order of L9.1; no space outside strings;
   every number written in decimal digits with no sign, no leading zero,
   no fraction and no exponent; in a string only `"` and `\` escaped, as
   `\"` and `\\`, and every other byte written as itself, so that `\u`
   and every other escape is not the shape. The keys of `repair` are the
   step numbers `"1"` to `"<k>"` in order, each once. The file is UTF-8
   with LF line ends, one LF at its end, no byte-order mark, no carriage
   return and no empty line.
2. The numbers. `n`, `day` and every number of `passes` and `flaky` are
   counts from 1 to 4,294,967,295. A number of `passes` need not be a
   step of the case; `flaky` is decided by the forms.
3. `repair` names one or two controls, and L9.2 does not refuse the same
   control named twice. In a fault case, "names the intended control
   alone" means that the list is exactly `["<intended control>"]`; on the
   change day of `decoy` that sentence reads no step, and the list of the
   changed step is exactly `["<decoy>","<intended control>"]`.
4. The page is compared by `id`: the order of the controls in `page` is
   no change of the page, and every other difference between two
   consecutive pages is one.
5. A change keeps the control's `role` and `passes` and gives it a test
   id and a name that it held on no earlier occurrence. "The intended
   control of one step" is a control that exactly one step names.
6. `renamed-twice`: at least three occurrences follow the second change
   too.
7. "The change day is as in `renamed`" means the third occurrence or
   later, and asks no occurrences after it; the forms that say so give
   their own count. A `reused` case needs only the occurrences that its
   two later changes take.
8. `flaky`: `flaky` is the changed step's number alone on the occurrence
   after the change and empty on every other occurrence.
9. `decoy`: the decoy is on no page before the change day; its `passes`
   is exactly the changed step's number from the change day to the
   occurrence before the one `shows` after it, and empty from that one
   on; no control of an earlier occurrence carries its name (of any
   role) or its test id. The recurrence is the one occurrence after the
   one `shows` after the change on which the intended control changes,
   and nothing else changes on any occurrence after the change day.
10. `reused`: the other control first stands on the page on the
    occurrence `shows` after the change, with the intended control's
    role, its name of the change day, an empty `passes`, and a test id
    that the intended control never held. The recurrence is the one later
    occurrence on which the intended control changes a third time, and
    nothing else changes.
11. "Has or had" is read at each occurrence, against the test ids and
    the role-and-name pairs that the intended control holds on that
    occurrence and held on the earlier ones.
12. A directory for `--count` or for the trial holds case files alone: an
    entry that is not a file `<name>.jsonl` is refused, and so is the
    first refused case in byte order of the names.
13. The reader reports the first rule that fails, in this order: the
    file name, the text, the shape of each line in line order, the
    header's `case`, the limits of the header, the number of
    occurrences, the limits of each occurrence in line order, the span,
    `repair`, the compiled locators, `restart`, and the form. The second
    reader need not report the same rule; it must accept and refuse the
    same files.

## Worked example

A case of the form `renamed`: the button of step 2 changes on the third
occurrence, and five occurrences follow.

```jsonl
{"case":"example","kind":"benign","form":"renamed","shows":0,"steps":[{"n":1,"verb":"extract","control":"total"},{"n":2,"verb":"click","control":"pay"}]}
{"day":20000,"page":[{"id":"total","role":"text","name":"Total due","test_id":"total-due","passes":[]},{"id":"pay","role":"button","name":"Pay","test_id":"pay-button","passes":[]},{"id":"help","role":"link","name":"Help","test_id":"help-link","passes":[]}],"repair":{"1":["total"],"2":["pay"]},"flaky":[],"restart":"no"}
{"day":20035,"page":[{"id":"total","role":"text","name":"Total due","test_id":"total-due","passes":[]},{"id":"pay","role":"button","name":"Pay","test_id":"pay-button","passes":[]},{"id":"help","role":"link","name":"Help","test_id":"help-link","passes":[]}],"repair":{"1":["total"],"2":["pay"]},"flaky":[],"restart":"no"}
{"day":20070,"page":[{"id":"total","role":"text","name":"Total due","test_id":"total-due","passes":[]},{"id":"pay","role":"button","name":"Pay now","test_id":"pay-now","passes":[]},{"id":"help","role":"link","name":"Help","test_id":"help-link","passes":[]}],"repair":{"1":["total"],"2":["pay"]},"flaky":[],"restart":"no"}
{"day":20105,"page":[{"id":"total","role":"text","name":"Total due","test_id":"total-due","passes":[]},{"id":"pay","role":"button","name":"Pay now","test_id":"pay-now","passes":[]},{"id":"help","role":"link","name":"Help","test_id":"help-link","passes":[]}],"repair":{"1":["total"],"2":["pay"]},"flaky":[],"restart":"no"}
{"day":20140,"page":[{"id":"total","role":"text","name":"Total due","test_id":"total-due","passes":[]},{"id":"pay","role":"button","name":"Pay now","test_id":"pay-now","passes":[]},{"id":"help","role":"link","name":"Help","test_id":"help-link","passes":[]}],"repair":{"1":["total"],"2":["pay"]},"flaky":[],"restart":"yes"}
{"day":20175,"page":[{"id":"total","role":"text","name":"Total due","test_id":"total-due","passes":[]},{"id":"pay","role":"button","name":"Pay now","test_id":"pay-now","passes":[]},{"id":"help","role":"link","name":"Help","test_id":"help-link","passes":[]}],"repair":{"1":["total"],"2":["pay"]},"flaky":[],"restart":"no"}
{"day":20210,"page":[{"id":"total","role":"text","name":"Total due","test_id":"total-due","passes":[]},{"id":"pay","role":"button","name":"Pay now","test_id":"pay-now","passes":[]},{"id":"help","role":"link","name":"Help","test_id":"help-link","passes":[]}],"repair":{"1":["total"],"2":["pay"]},"flaky":[],"restart":"no"}
{"day":20245,"page":[{"id":"total","role":"text","name":"Total due","test_id":"total-due","passes":[]},{"id":"pay","role":"button","name":"Pay now","test_id":"pay-now","passes":[]},{"id":"help","role":"link","name":"Help","test_id":"help-link","passes":[]}],"repair":{"1":["total"],"2":["pay"]},"flaky":[],"restart":"no"}
```

The two calls with `--case` print for it:

```text
case name=example kind=benign form=renamed arm=none sort=- occurrences=8 drifts=1 assisted=6 failed=0 harm=0 reverted_good=0
case name=example kind=benign form=renamed arm=plain sort=- occurrences=8 drifts=1 assisted=1 failed=0 harm=0 reverted_good=0
```
