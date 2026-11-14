# halvo-sample

`halvo-sample` writes and checks the labelled sample of the workflow run. Its contract is
contracts.md C9 to C12 of the run ledger. Run it as
`cargo run -p halvo-sample -- <command> <arguments>`. The commands are `compose`, `split`,
`build`, `check-labels`, `input-copy`, `leak` and `mask-layers`.

The library holds the readers and writers of a body: the tier manifest (`manifest`), the label
rows (`labels`), the script manifest (`manifest`), the script with its rules S1 to S15
(`script`), and the site names, title tokens, site-masked titles and templates of C12.4
(`text`).

## The draws of `build`

Each stream has one ChaCha20 generator. Its key is the SHA-256 of the text
`halvo-sample build <N> <stream id>`, with the seed N in decimal and the script stream id.
A draw takes the next 64-bit word w. A choice among n values is the value with the number
w mod n, counted from 0.

The items and marks of the stream are taken in the order of their instant. A mark comes before
an item at the same instant, and a start mark before an end mark. For each item, in that
order, `build` draws:

1. The seconds of the keystroke and click records. The list is every whole second after the
   item's `start` and before its `end`, in rising order. Each draw takes the entry at index
   w mod (entries left) and removes it from the list. `build` makes `keys` + `clicks` draws.
2. One word for each keystroke record. The class is w mod 7, in the order `printable`,
   `editing`, `navigation`, `enter`, `tab`, `shortcut`, `other`. The first `keys` seconds of
   step 1, in the order of the draws, are keystroke records. The other seconds are clicks.
3. The seconds of the scroll records, drawn from a new copy of the same list, as in step 1.
4. One word for each scroll record. Its `count` is 1 + (w mod 20).

`build` then sorts the records of the stream by instant. At one instant the order is
`run_mark`, `foreground_app`, `window_title`, `keystroke`, click, scroll. For each record in
that order it draws two words, w1 and w2, for the record's id. The id is a UUID of version 7:

- the first 48 bits hold the millisecond of `at`, rounded down;
- then the version 7;
- then the top 12 bits of w1;
- then the variant bits `10`;
- then the top 62 bits of w2.

`seq` counts the stream's records from 0, across its day files. It counts only the records
that `build` writes: a record that cannot be made takes no number.

`build` takes script files whose names end in `.jsonl` and do not start with a dot. It refuses
any other SCRIPT argument with `script`, a directory among them, so a caller that passes every
entry of a scripts directory skips no file.

## The script manifest and `check-labels`

Each line of the script manifest ends with the key `seed_category`. `build` writes it true when
an episode whose category is not `other` names the step in one of its ranges, and false
otherwise. `check-labels` knows a seed-category step by this key alone, not by the label rows.
So a body that lost every row of a seed task fails with `FAIL outside` lines under the
generated rule. A range of such an episode whose step is marked false fails with `FAIL seed`
under both rules.

`check-labels` follows no symbolic link. Each entry of `streams/` that is a link gives
`FAIL link <stream id>`, and these lines come first. A `streams` directory that is itself a
link is refused with `batches`. Each range is held against every earlier range whose end lies
after its start, and the later row of each overlapping pair gets its `FAIL row` line.

## `leak`

`leak` refuses a PATH argument that is a symbolic link with `path`, also when separators or
`.` follow its name. Inside a walk it follows no link. A stream directory that is a link in
the tier or in a development body is refused with `tier-files` or `body`. A `HIT` line names a
file as the PATH argument followed by `#<n>`: n is the number of the file among the files that
the walk of that argument searches, in byte order of name, from 1. No name below the argument
is printed, because a file's name may be a service's name.

## What later units must know

- `compose --ranges` reads no file. Its fifteen lines are constants of the command.
- `input-copy` writes `layers/instants.jsonl` only when the input set lists a layer.
- The records of an input copy, its run marks among them, are those that the seam's fixture
  transport keeps for the input set. `input-copy` adds and moves no record.

## The draws of `compose`

`compose` has one ChaCha20 generator. Its key is the SHA-256 of the text
`halvo-sample compose <seed> <sides>`, for example `halvo-sample compose 7 seen,unseen`.
It draws the same way as `build`.

`compose` first puts the services into two halves, and draws nothing for this. For each
category, in the order of C0.5 and then `other`, it sorts the services of the category by
kind, native first, and then by name. A canary's service is left out. The services go to
half 0 and half 1 in turn. Half 0 holds the streams `s001` to `s<n>`, and half 1 the next n
streams, where n is `streams` / 2.

For half 0, then half 1, `compose` plans the tasks:

1. For each stream k of the half (k counted from 0), it places a `one_off` task in the seed
   category with the number 3k mod 10, a `twice` task in the category (3k + 1) mod 10, and a
   `marked` task in the category (3k + 2) mod 10. Each placement draws one word to choose
   among the services of the category that the stream does not use yet. A `twice` task on a
   native service also blocks, for its stream, each other service of the category in the half
   with the same app.
2. For each stream, it places one `near_miss` task in the first category, starting at the
   number (3k + 3) mod 10 and counting up, where the stream has a free service. It draws as in
   step 1.
3. For each seed category, it draws the rest of the near-miss routines, then `planted` / 2
   planted routines. Each draws slots without repetition from the free slots. A slot is one
   stream and one service. The slots are listed by stream and then by service, in the order of
   the half.
4. For each stream, it draws one service of the category `other` of the half for the stream's
   `non_seed` task.

Then, for each task in the order in which it was placed, `compose` draws:

1. a flow, for a `non_seed` task only;
2. one value for each name of the service's `parameters`;
3. the number of episodes: 3 to 6 for a planted routine, 3 to 5 for a near-miss routine and 3
   to 8 days for a non-seed routine (no draw for the other kinds);
4. its days without repetition from 1 to 93. A marked task has one day.

Then, for each stream in order, `compose` draws:

1. one day for each canary, among the stream's days that hold a task;
2. for each such day, in rising order:
   1. a shuffle of the day's episodes, without the marked ones;
   2. a length for each stretch of each episode;
   3. the number of noise items, and for each one its page, its length and its place among
      the episodes;
   4. the canary noise items of the day, the same way;
   5. the start of the day;
   6. the place of the marked run, on the day of the marked task.

`compose` then lays out the day. At an episode that has another unit after it, it draws whether
to cut the episode, and where. When it cuts, the next unit goes into the cut. For each stretch,
in the order of time, it draws the counts of keys, clicks and scrolls. After each unit, it draws
the gap. In the marked run, it draws the gap before each episode, the length of each stretch,
the counts of each stretch and the gap before the end mark.

The step id of a page with one choice of values is `p` and the first 15 hex digits of the
SHA-256 of `<service>`, LF, `<page id>`, LF, `<path>`.
