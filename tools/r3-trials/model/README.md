# The model cells (R3, unit U0.5)

This trial finds out which small language models fit the memory budget of an agent on the
person's own machine (contracts.md A13.5 of the agent-setup run). It holds the first form of the
model child, the binary `r3-model`, which loads one 4-bit GGUF file on llama.cpp on the CPU and
answers request lines with reply lines (A12.2). It measures each model's child under the memory
cap of `crates/halvo-cap` with `measure-peak`. Its result is `docs/r3/trials/model-cells.md`.

No product build reads this directory: it has a workspace and a `Cargo.lock` of its own, and the
root workspace does not list it.

## Files

| File | What it is |
|---|---|
| `weights.txt` | One line per model, written before any cell: its token, group, file name, SHA-256, size in bytes, address at a pinned commit of its publisher's own repository, and licence |
| `fetch.txt` | The line of the build tool cmake, from Kitware's own release |
| `fetch.sh` | Fetches one model's file by its token and checks it; brings cmake on macOS; checks the form of both files |
| `src/` | The child `r3-model` |
| `grammars/action.gbnf` | The action grammar of A12.4 |
| `settings.toml` | The starting values of A12.5, which the cells pass to the child |
| `prompts/action.txt`, `prompts/reply.txt` | The two prompts of the cells |
| `run.sh` | Runs the cells of one model on the machine it is on |
| `fits.py` | The budget arithmetic: prints the bound lines and the verdict line from the cell lines |
| `.gitattributes` | Checks out every file of the trial with LF line ends, also where `core.autocrlf` is true |

## The models

A model enters `weights.txt` only when its publisher's own repository on Hugging Face holds a
4-bit GGUF file of it that downloads with no account, under MIT or Apache-2.0. The address of
each file pins a commit, and the size and SHA-256 are the ones that the publisher's file listing
at that commit states (the `lfs` size and SHA-256 of the host's tree listing; the host's other
64-hex value is a hash of its own storage). The listings were read on 2026-11-11.

| Token | Group | Repository at the pinned commit | File | Licence |
|---|---|---|---|---|
| `minicpm5_2b` | agents | `openbmb/MiniCPM5-2B-GGUF` | `MiniCPM5-2B-Q4_K_M.gguf` | Apache-2.0 |
| `granite_4_2_3b` | agents | `ibm-granite/granite-4.2-3b-GGUF` | `granite-4.2-3b-Q4_K_M.gguf` | Apache-2.0 |
| `ministral_3_3b` | agents | `mistralai/Ministral-3-3B-Instruct-2512-GGUF` | `Ministral-3-3B-Instruct-2512-Q4_K_M.gguf` | Apache-2.0 |
| `s5_nano_4b` | routing | `maritaca-ai/S5-nano-GGUF` | `S5-nano-Q4_K_M.gguf` | Apache-2.0 |
| `gemma_4_e2b` | routing | `google/gemma-4-E2B-it-qat-q4_0-gguf` | `gemma-4-E2B_q4_0-it.gguf` | Apache-2.0 |
| `gemma_4_e4b` | routing | `google/gemma-4-E4B-it-qat-q4_0-gguf` | `gemma-4-E4B_q4_0-it.gguf` | Apache-2.0 |

Three models of the contract left the list. `qwen3_5_2b` and `qwen3_5_4b`: the Qwen
organisation's repositories of Qwen3.5-2B and Qwen3.5-4B are Apache-2.0, but none of its 54 GGUF
repositories is one of Qwen3.5, so no 4-bit file of the publisher's own exists, and a file that a
third party converted is not taken. `ternary_bonsai_2_9b`: its publisher's repository
`prism-ml/Ternary-Bonsai-2-9B-gguf` is Apache-2.0 and holds 2-bit and ternary files and no 4-bit
one, as the routing run found (`docs/r6/trials/routing-group.md`).

The pages of all three name Apache-2.0. The result file holds no licence line for them, because
the licence line of a model names the one file that the SHA-256 of its `weights` line pins
(A13.5), and none of the three has a `weights` line; their `notrun` lines give the cause.

## Fetch

```sh
sh tools/r3-trials/model/fetch.sh TOKEN
```

The script reads the line of `TOKEN` in `weights.txt`, fetches the file from its address with
curl under the client name `curl`, over https only, with no token, cookie or account, and
compares the size and the SHA-256 of the bytes with the line. A redirect to another host of the
same publisher is followed; the hash decides, not the host. On macOS the file is kept as
`~/Library/Caches/halvo-trials/weights/<sha256>`, outside every repository, so that other
trials and runs on the machine read the same file and nothing is fetched twice, and
`target/r3-weights/<file>` of the repository is a symbolic link to it. On another system, such
as a hosted Windows runner, the file is `target/r3-weights/<file>` itself. A file that is there
and matches is not fetched again. The script removes nothing.

`sh tools/r3-trials/model/fetch.sh --check` checks the form of every line of `weights.txt` and
`fetch.txt`, the licence of each model among them, and `--self-test` runs those checks on
fixtures.

## Build

The child links llama.cpp through the crate `llama-cpp-2` 0.1.161, whose build needs cmake and
libclang. The hosted runners have both. On a Mac with no cmake, `fetch.sh --cmake` fetches
Kitware's release `cmake-4.4.5-macos-universal.tar.gz` of the line of `fetch.txt` (the newest
release that was at least seven days old on 2026-11-11, published 2026-10-27; its SHA-256 is
the one of Kitware's `cmake-4.4.5-SHA-256.txt`) and unpacks it under `target/r3-tools/` of the
repository. Nothing is installed outside `target/`.

The loader is built through `run.sh` alone. Before its first cell, `run.sh` runs

```sh
GGML_METAL=OFF cargo build --release --locked -p halvo-cap --bin measure-peak
(cd tools/r3-trials/model && GGML_METAL=OFF cargo build --release --locked)
```

with the cmake of `fetch.sh --cmake-dir` first on `PATH` when none is on it, and a build that
fails is a `notrun` line with `cause=build`. A second call finds both binaries up to date. On
Apple silicon llama-cpp-2 turns on the `metal` feature of llama-cpp-sys-2 whatever its own
features say, and only the variable `GGML_METAL=OFF` keeps the Metal backend out of the build;
the repository tracks no cargo configuration file that could set it (the check 40-r6-policy
refuses one). A build of `r3-model` made another way, with no such variable, has the Metal
backend, and the child refuses to run on it: it stops when llama.cpp says that the build can
offload to a GPU.
The release binary still names `Metal.framework` and `MetalKit.framework` among its libraries,
because the build script links them for that feature, and imports no symbol of either.

## The child

```
r3-model --weights FILE --settings FILE [--requests FILE --replies FILE]
```

The arguments, the `ready` line and the request and reply lines are those of A12.2. With the two
files the child reads its request lines from the first, appends its `ready` line and each reply
line to the second, and exits after the last request; without them it reads standard input and
writes standard output. What it does, in this order:

1. It reads and checks the settings file: exactly the keys of A12.5.
2. It takes the model lock of halvo-cap before it reads the weights, and exits 2 with
   `REFUSED lock: held` when the lock is held. It holds the lock until it exits.
3. It reads the tensor table of the GGUF file and refuses a file that has a tensor above 4 GiB
   (cause `weights`): llama.cpp's lazy mode `auto` maps a marked tensor above that size whatever
   the load mode, and llama-cpp-2 0.1.161 has no switch for that mode.
4. It refuses to go on when the build of llama.cpp can offload to a GPU.
5. It loads the weights with no layer on a GPU and llama.cpp's load mode `none`, so that the
   weights are read into the process and not mapped, and creates one context of the settings'
   size (8,192 tokens) with a batch of 2,048 tokens, a micro-batch of 512, 4 threads, a key and
   value cache of type f16 and no offload of the cache or of operations. It stops when llama.cpp
   puts another context or batch size in force. `load_ms` is the time from the start of the
   load to the ready context.
6. For each request it cuts the snapshot at a line end at `snapshot_tokens` tokens, puts it after
   the prompt between the lines `<<<DATA` and `DATA>>>`, builds the text with the model's own
   chat template (a system turn and a user turn), clears the cache, decodes the text and samples
   greedily at temperature 0, through the action grammar or as free text, until an end token or
   the level's token limit. `first_token_ms` is the time from the start of the request to the
   first sampled token, and `ms` the time to the end of the reply.

   llama-cpp-2 0.1.161 reaches llama.cpp's template engine alone, which knows a fixed set of
   template forms and does not apply the chat template of Gemma 4 (its turns start with
   `<|turn>`). For such a model the child sends the system text, an empty line and the user turn
   as plain text, parses no special token in it, and says so on standard error once per request.
   The cells of `gemma_4_e2b` and `gemma_4_e4b` ran so: they show the footprint and the speed of a
   request of that size, and their replies are no test of the model.

The child starts no process, does not exec, maps no weights, keeps nothing in purgeable or
reusable memory and puts no layer on a GPU: the loader rules of the workflow run's C23.10, without
which a run under `measure-peak` is no measurement. llama.cpp's own log on standard error says
`load_mode = none` and names the buffers of the weights (`CPU` and `CPU_REPACK`, none `Mapped`).
A refusal prints one line, because llama.cpp's log is off until the checks have passed.

The child links no network code. `tools/r2-model-spike/check-binary.sh` of the workflow run, as
its commit 896950c holds it, reads the release binary with `nm` and `otool` on macOS and exits 0
only when it imports no socket call, no TLS or URL-loading symbol and no Metal symbol and holds no
symbol of llama.cpp's HTTP or download code. No tool shows it on Windows.

## The cells

```sh
sh tools/r3-trials/model/run.sh --model TOKEN [--prompt action|reply]
```

A cell is one model, one machine and one prompt: `measure-peak --limit 8589934592 --runs 5 --
r3-model ...`, where each run loads the model and answers the one request of the prompt at the
level `medium`. `run.sh --help` gives the line forms. The machine is `mac` on macOS outside CI
and `hosted-win` on Windows in GitHub Actions; on any other machine the cell is a `notrun` line
with `cause=machine`. On `mac` every cell runs through the machine's timed-run helper, one at a
time. `hosted-mac` (macos-15, 7 GB) is not used.

`action.txt` holds a boundary text, a task line and the snapshot of `/pay` of seed 1
(`tools/r3-trials/web/snapshots/pay-seed-1.txt`, byte for byte); its request uses the grammar
`action`. `reply.txt` holds the same boundary text, a task line and the first message of seed 1
that the reply rule selects; its request uses the grammar `text`. Both hold invented values of
the fixture (contracts.md A0.9), and no line that `run.sh` prints quotes them: the reply text
stays in a temporary directory.

`tokens_per_s` is `tokens_out` times 1,000 over the time from the first reply token to the end of
the reply, rounded down.

What the cells showed that a later unit needs. MiniCPM5-2B's own chat template starts a
reasoning block (`<think>`), so its reply to `reply.txt` ran to the limit of 512 tokens in every
run with the reasoning first; llama.cpp's template engine of 0.1.161 has no way to turn that
off. The action grammar holds every model to one action object, and the reply ends when the
object is complete.

## The budget

```sh
python3 -I tools/r3-trials/model/fits.py           # prints the bound lines and the verdict line
python3 -I tools/r3-trials/model/fits.py --check   # exit 1 when the file's lines differ
python3 -I tools/r3-trials/model/fits.py --write   # puts them in docs/r3/trials/model-cells.md
```

The script reads `browser_bound_bytes=` of the verdict line of
`docs/r3/trials/browser-profile.md`, so when that bound changes, the fits and the verdict are
computed again from the stored peaks by one command and no new run. The rule is A13.5's: the
bound of a prompt is the median of its five peaks times 1.25 rounded up to 8 MiB; the bound of a
model on a machine is the larger of its two prompts' bounds; the budget is 8,589,934,592 bytes
less the browser bound with one step of 8,388,608 bytes added, less 268,435,456 bytes for the
runner. `fits` is `not-measured` when a prompt has fewer than five cell lines, a run was capped,
or a median peak is below the file's size. A cell line whose `file_sha256` is not the SHA-256 of
its model's line in `weights.txt`, and a run number that stands twice for one model, machine and
prompt, stop the script with `REFUSED source`.

A bound read again can stand one 8 MiB step below the file's. A reader's five runs of the
`minicpm5_2b` action cell on `mac`, at a 1-minute load of 301, gave a median 0.14 per cent below
the file's and so a bound of 2,826,960,896 bytes against the file's 2,835,349,504; `fits=yes`
holds for both. A bound at a rounding edge cannot be reproduced exactly, so a comparison of two
bounds allows one step, and a bound of the result file stays the median of five runs, because
one run of that reader's read 10 per cent low under this machine's load.

## How the cells of unit U0.5 ran

On `mac` the cells ran in three rounds, each cell one call of the timed-run helper. The result
file holds the last round, which ran on the release binary of commit 6344f80, built with the
cargo configuration file that c3a7186 took out (the same `GGML_METAL=OFF`); later commits change
no cell's run or line; the captures of the earlier rounds stay in the builder's evidence.
The first round ran on the binary of commit 854151b: there the two Gemma 4 models answered no
request (llama.cpp's template engine returned an error for their template), which commit e48a9b8
turned into the plain form above; one cell of `ministral_3_3b` stopped with a syntax error
because `run.sh` was changed while it ran; and two captures of `ministral_3_3b` lost their cell
lines because the capture helper's `sed` stopped at a byte of llama.cpp's log that is no UTF-8. The
second round ran on the binary of e48a9b8; commit 6344f80 changed the order in which the child
frees its lock at exit and the error paths of `run.sh`, so the third round ran the cells again on
that binary.

On `hosted-win` four scratch runs reached the cell steps. In run 43094722413 (commit ed471a5) the
child did not build on MSVC: its log callback took the level as `u32`, which bindgen makes `i32`
there (fixed in e48a9b8). In that run and in run 43099238425 (commit e48a9b8) `fetch.sh` refused
every line of `weights.txt`, because the checkout with `core.autocrlf=true` had given the file CRLF
line ends (fixed in 250f84b by the trial's `.gitattributes`). Run 43099769860 (commit 250f84b)
ran the twelve cells. Commit c3a7186 took the cargo configuration file out of the tree and made
`run.sh` build the loader, so run 43135922738 (commit c3a7186) ran the twelve cells again, with
`run.sh` building both binaries in its first call; the result file takes them from that run. Run 43094693697 (commit ed471a5)
ran the repository's own checks alone, trimmed, and was green in both jobs.

Each cell keeps its temporary directory under `TMPDIR` or `/tmp`, with the request, the child's
replies and the raw output of `measure-peak` and of the child, and `run.sh` removes nothing. The
request holds the invented values of the prompt file and the replies hold the model's text, so a
search of a machine for fixture values finds them there.
