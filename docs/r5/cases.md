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
to 32 bytes of `a` to `z`, `0` to `9` and `-`. `docs/r5/cases.md`
(U0.3's) repeats this contract and L7.4's line and its two author calls
with one worked example. Line 1 is the header, every later line one
occurrence. A case file is at most 1,048,576 bytes, and the reader tests
the size before it reads the file.

```
{"case":"<name>","kind":"benign"|"fault","form":"<form>","shows":0|1|2,"steps":[{"n":<count>,"verb":"click"|"fill"|"extract","control":"<control id>"}...]}
{"day":<day>,"page":[{"id":"<control id>","role":"<role>","name":"<text>","test_id":"<text>","passes":[<count>...]}...],"repair":{"<n>":["<control id>"...]},"flaky":[<count>...],"restart":"yes"|"no"}
```

A case file is a text file of L0.4; a file that is not is refused. A
number is written in decimal digits with no sign, no leading zero (the
one digit 0 is allowed), no fraction and no exponent. In a string only
`\"` and `\\` are escapes; every other byte stands for itself. The keys
of `repair` are the step numbers in ascending order, each once.

## Meaning (L9.2)

`steps` is the task: 1 to 8 steps numbered from 1 with no gap, each
aimed at one control of the page, its intended control, named by
`control`. Two steps may aim at one unchanged control. A `<control id>`
is 1 to 16 bytes of `a` to `z`, `0` to `9` and `-`; it is the control's
identity for scoring, and no locator may read it. `day` is a whole
number from 1 to 4,294,967,295, the scripted day of the occurrence, and
rises strictly from line to line. `page` is the list of controls on that
day, at most 16, each `id` once, with the intended control of every step
among them; `role` is one of `button`, `link`, `textbox`, `checkbox`,
`combobox` and `text`; `name` and `test_id` are 1 to 64 bytes of
printable ASCII and hold no personal value; `passes` may be empty.
Printable ASCII is 0x20 to 0x7e, the space included, counted on the
value and not on its escaped spelling. A number of `passes` is a whole
number from 1 to 4,294,967,295 and need not be a step of the case.

The compiled playbook of a case is made from the page of the first
occurrence: step n gets two locators, `test_id` with the intended
control's test id, then `role` with its role and name. Each of a step's
two compiled locators must resolve alone on the first page, not only the
step as a whole. A locator resolves on a page when exactly one control
matches it; a step's locators are tried in order and the step acts on
the control of the first that resolves. A step's condition holds when it
acted on its intended control, or on a control whose `passes` list holds
the step's number: such a control is a decoy that the step's own check
cannot tell from the right one. A step fails with the cause
`step_failed` when no locator resolves or when its condition does not
hold after it acted.

`repair` holds, for every step and written under its number as a string,
the one or two controls (two only where L9.3 allows it) that the stub
repairer names when that step fails, in order; the last of them is
always the step's intended control. The repairer is asked when a step
fails with `step_failed`, and never at a `timeout`: the runner acts on
the first named control, then on the second when the condition did not
hold; when the condition holds the step is recovered, and the proposal
is the served step with the locator `role` of that control's role and
name put first and with that control's description, as the agent-setup
run builds a description from a snapshot line (AG-A3.2). Because the
last named control is the intended one, every occurrence of a case that
the reader accepts completes, in every arm. `flaky` lists steps that
fail once on that day with the cause `timeout`, before any locator is
tried; the step is then tried again and behaves as the page says. A
number of `flaky` is a whole number from 1 to 4,294,967,295. `restart`
`yes` makes the gate host close and open the host and the store before
the occurrence; the trial's harness, which has neither, reads the key
and does nothing. A case has 8 to 16 occurrences and spans at least
`calendar_days_min` days (240) from its first to its last, and at least
one occurrence more than 93 days after the first holds `restart` `yes`,
so that run records of the first occurrences pass their 93 days at an
open of the store inside the case. The reader reads `calendar_days_min`
from the targets file, as L8.1 says.

## The forms (L9.3)

`shows` is 0 in a benign case and 1 or 2 in a fault case. A change of a
control gives it a new test id and a new name on the same day. A change
gives the control a test id and a name that it held on no earlier
occurrence, and keeps its role and `passes`. The control that changes is
named by exactly one step. The first change is at the third occurrence
or later. The occurrences that must follow are those that the form's own
bullet states; `reused` states none beyond its recurrence. Each case has
exactly one form. A reader checks exactly the rules that this paragraph,
the bullets and the paragraph under them state, and no other property of
a form; the page changes only as the form's bullet says, and `flaky` is
empty outside the form `flaky`. Two pages are the same when they hold
the same controls by `id` with the same role, name, test id and
`passes`; the order of `page` is not compared.

- `renamed` (benign): the intended control of one step changes once, at
  the third occurrence or later, and at least three occurrences follow.
- `renamed-twice` (benign): as `renamed`, and the same control changes a
  second time, at least three occurrences after the first change, and at
  least three occurrences follow the second change.
- `alternates` (benign): the change day is as in `renamed`, with at
  least four occurrences after it; from it on the page shows the new and
  the old form of the control in turn, the new one first.
- `flaky` (benign): as `renamed`, and the changed step is flaky on the
  occurrence after the change. `flaky` holds the changed step's number
  alone on the occurrence after the change and is empty on every other
  occurrence.
- `decoy` (fault): the change day is as in `renamed`, with at least
  `shows` + 2 occurrences after it. From the change day on the page also
  holds a decoy, a control of the same role whose `passes` is exactly
  the changed step's number on that day and on every occurrence before
  the one named next, and empty from that one on. On the occurrence that
  is `shows` occurrences after the change, and on every later one, the
  decoy passes nothing and stays on the page. On the change day the
  changed step's `repair` is exactly the decoy and then the intended
  control. The decoy's name and test id are new on the change day. No
  control of any role carries the decoy's name or its test id on an
  occurrence before the change day, and the decoy stands on no page
  before it. On a still later occurrence, the recurrence, the intended
  control changes again while the decoy keeps its name. The decoy's
  recurrence is strictly after the occurrence `shows` after the change.
  The recurrence is the one occurrence after it on which the intended
  control changes; no other occurrence changes the page.
- `reused` (fault): the change day is as in `renamed`. On the occurrence
  that is `shows` occurrences after it, the intended control changes
  again, and its name of the change day now stands on another control of
  the same role, which first stands on the page on that occurrence and
  passes nothing. The recurrence is the one occurrence after that
  occurrence on which the intended control changes; no other occurrence
  after that one changes the page. That other control carries a test id
  of its own: a test id that no other control of the case holds or held
  on any occurrence. A case in which it shares a filler's test id is
  refused. On a still later occurrence, the recurrence, the intended
  control changes a third time while that other control keeps the name.

In a benign case `plain` completes with `harm` 0 and `assisted` equal to
`drifts`, and `none` has `assisted` of at least `drifts` + 2. In a fault
case `harm` of `plain` is at least 3 for `decoy` and at least 2 for
`reused`. In every case, benign or fault, on every day and for every
step, `repair` is the list of one entry, the step's intended control;
the one exception is the changed step on the change day of `decoy`,
whose list is exactly the decoy and then the intended control, which is
the rule of the bullet `decoy` on its `repair`. A list that names a
control twice, a second control on any other day, or another step's
two-entry list on the decoy's change day is refused, and so is a list of
the changed step on that day that names another control first. No
control but a step's intended one carries a test id, or a role with a
name, that the intended control has or had; the one exception is the
other control of `reused`, which carries the name of the change day and
a test id of its own. So the stub repairer is wrong once, on the change
day of `decoy`, and at the recurrence only a stored locator can lead a
step to the wrong control. The reader computes these with its own arms
`none` and `plain` and refuses a case that misses them. Changes that the
rule may not learn are no change of a case and are tested as refusals
(L3.9): a new step, a changed condition, another frame or origin, a
credential field.

## The refusals (L9.4)

The reader's refusals, each one line `REFUSED case: <file name>: <rule>`
with status 2, which a check script sends to its artifacts (L0.9): a
file that breaks a rule of L9.1, its size, which the reader tests before
it reads the file, its text, or the shape of a line; a header `case`
that is not the file's name without `.jsonl`; a count or a form of L9.2
outside its limits; an occurrence whose `repair` lacks a step or names a
control that the page lacks, or whose last named control is not the
intended one, or a `repair` list that L9.3 refuses; a first page on
which a compiled locator does not resolve to its intended control; a
case with no `restart` `yes` more than 93 days after its first day; a
case that breaks a rule of its form in L9.3. A refusal names a rule that
the file breaks. When a file breaks more than one rule the reported rule
is not fixed; the two readers are compared by accept and refuse alone.
Two rules cannot be broken alone by a case that passes the others: that
a step's compiled `test_id` locator resolves to its intended control on
the first page, and that its compiled `role` locator does; a case that
breaks either also breaks the rule of L9.3 that no control but a step's
intended one carries a test id, or a role with a name, that the intended
control has or had. A directory of cases holds files `<name>.jsonl`
alone; another entry is refused with `case`. The two readers, U0.3's and
U2.2's, hold the same list: a file that one accepts and the other
refuses is a defect of a reader, which `50-r5-drift.sh` shows, and never
a reason to change a case. Scoring is the harness's, from `id` alone. A
learner, the handlers and the step never read `id`, `control`, `passes`,
`kind`, `form` or `shows`.

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

L9.1 to L9.4 now state the readings that this section held before.
Two things remain open, and this reader settles them so.

1. "Has or had" is read at each occurrence, against the test ids and
   the role-and-name pairs that the intended control holds on that
   occurrence and held on the earlier ones.
2. The reported rule, which L9.4 leaves open when a file breaks more
   than one rule. This reader tests the rules in this order and reports
   the first that fails: the file name, with the bytes of `<name>` in
   it; the size; the text; the shape of each line in line order; the
   header's `case`; the limits of the header; the number of
   occurrences; the limits of each occurrence in line order; the span;
   `repair` in line order; the compiled locators; `restart`; and the
   form: `kind` and `shows`, `flaky` outside its form, the rules of the
   form's bullet in an order of their own, the controls that carry what
   an intended control has or had, the test id of the other control of
   `reused`, `repair` alone, and the counts of the paragraph. When the
   first difference between two pages is not a change of exactly one
   step's intended control, it reports that the page changes other than
   the form says, and so does `alternates` for a difference outside the
   changed control. In a directory it reports the first refused file in
   byte order of the names.

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
