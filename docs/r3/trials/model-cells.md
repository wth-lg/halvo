# Model cells (R3, unit U0.5)

Each model of `tools/r3-trials/model/weights.txt` was loaded by the trial loader `r3-model`
(llama.cpp of the crate llama-cpp-2 0.1.161 on the CPU, no layer on a GPU, the weights read into
memory, under the model lock of halvo-cap) and answered the two prompts of
`tools/r3-trials/model/prompts/` at the level `medium`, one request per run, under `measure-peak
--limit 8589934592 --runs 5` of `crates/halvo-cap` as main holds it. On `mac` (M3 Max, 64 GB) each
cell went through the timed-run helper, one at a time, on the loader of commit 6344f80, which the
later commits change in no cell's run or line; its `timed` line gives the helper's start and end
and the 1-minute load when it started. On `hosted-win` (windows-2025, 4 logical CPUs,
17,174,360,064 bytes) the cells ran in the run of the `hosted` line, at commit c3a7186, which
fetched each file itself in 6 to 1,302 s. `hosted-mac` (macos-15, 7 GB) is not used. More: README.md.

The budget is 8,589,934,592 - (1,216,348,160 + 8,388,608) - 268,435,456 = 7,096,762,368 bytes,
from `browser_bound_bytes=1216348160` of `docs/r3/trials/browser-profile.md`; `python3 -I
tools/r3-trials/model/fits.py --write` computes the `bound` and `verdict` lines from the `cell`
lines, again with no new run when that bound changes. The verdict rests on the peaks alone.

Three models of the contract left the list (their `notrun` lines): `qwen3_5_2b` and `qwen3_5_4b`,
because none of the Qwen organisation's 54 GGUF repositories holds Qwen3.5, and
`ternary_bonsai_2_9b`, because its publisher's repository holds no 4-bit file. llama.cpp's
template engine of llama-cpp-2 0.1.161 does not apply Gemma 4's chat template, so the child sent
the text of `gemma_4_e2b` and `gemma_4_e4b` as plain text: their cells show the footprint and the
speed of a request of that size, and their replies test nothing. MiniCPM5-2B's own template opens
a thinking block that this engine cannot turn off, so its reply ran to 512 tokens on `mac`.

Choices left open, as built: 4 threads, a batch of 2,048 and a micro-batch of 512 tokens; the
model's own chat template, a system and a user turn; the verdict in the order of A13.5's table;
`tokens_per_s` is `tokens_out` x 1,000 over the time from the first reply token to the end.

What the lines cannot show. On `mac`, `check-binary.sh` of the workflow run's commit 896950c found
no socket call, no TLS, URL-loading or Metal symbol and no HTTP or download code in the release
binary of `r3-model`; on `hosted-win` no tool shows that no network code is linked into the loader.
`capped=no` says that no read was above the limit (on Windows, that no report of a refused
allocation arrived), not that the child held no more. Not counted on macOS: other processes, memory
before an exec, mapped pages, purgeable and reusable memory, kernel messages and socket buffers (the
child starts none, does not exec, maps no weights); on Windows: other processes, mapped files,
sections and broker processes. The `mac` cells started at 1-minute loads of 82 to 443: their times
are upper bounds and their speeds lower bounds. In a reader's two `minicpm5_2b` action cells on
`mac`, one run of ten answered 2 tokens where the other nine answered 29, under greedy sampling.
```text
hosted machine=hosted-win run=43135922738 sha=78cb1d8a3482acc10981cbdef29038d47d5b1b0e
runtime crate=0.1.161 backend=cpu load=none sampling=greedy ctx=8192 batch=2048 threads=4 cache=f16
licence item=minicpm5_2b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=granite_4_2_3b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=ministral_3_3b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=s5_nano_4b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=gemma_4_e2b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=gemma_4_e4b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=llama.cpp@0.6.0 spdx=MIT read=2026-11-11 ok=yes
licence item=llama-cpp-2@0.1.161 spdx=MIT read=2026-11-11 ok=yes
licence item=llama-cpp-sys-2@0.1.161 spdx=MIT read=2026-11-11 ok=yes
licence item=enumflags2@0.7.12 spdx=MIT read=2026-11-11 ok=yes
licence item=libc@0.2.191 spdx=MIT read=2026-11-11 ok=yes
licence item=once_cell@1.21.4 spdx=MIT read=2026-11-11 ok=yes
licence item=pin-project-lite@0.2.17 spdx=MIT read=2026-11-11 ok=yes
licence item=serde_core@1.0.229 spdx=MIT read=2026-11-11 ok=yes
licence item=serde_spanned@1.1.3 spdx=MIT read=2026-11-11 ok=yes
licence item=thiserror@2.0.21 spdx=MIT read=2026-11-11 ok=yes
licence item=toml@1.2.1+spec-1.1.0 spdx=MIT read=2026-11-11 ok=yes
licence item=toml_datetime@1.1.3+spec-1.1.0 spdx=MIT read=2026-11-11 ok=yes
licence item=toml_parser@1.1.5+spec-1.1.0 spdx=MIT read=2026-11-11 ok=yes
licence item=tracing@0.1.44 spdx=MIT read=2026-11-11 ok=yes
licence item=tracing-core@0.1.36 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-link@0.2.1 spdx=MIT read=2026-11-11 ok=yes
licence item=windows-sys@0.61.2 spdx=MIT read=2026-11-11 ok=yes
licence item=winnow@1.0.5 spdx=MIT read=2026-11-11 ok=yes
notrun model=qwen3_5_2b machine=mac cause=no_own_4bit_file
notrun model=qwen3_5_2b machine=hosted-win cause=no_own_4bit_file
notrun model=qwen3_5_4b machine=mac cause=no_own_4bit_file
notrun model=qwen3_5_4b machine=hosted-win cause=no_own_4bit_file
notrun model=ternary_bonsai_2_9b machine=mac cause=no_own_4bit_file
notrun model=ternary_bonsai_2_9b machine=hosted-win cause=no_own_4bit_file
timed model=minicpm5_2b machine=mac prompt=action start=2026-11-11T21:42:48Z end=2026-11-11T21:46:30Z load1=443.02
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=1 peak_bytes=2262124632 capped=no load_ms=6950 first_token_ms=26273 tokens_per_s=1 tokens_in=324 tokens_out=29
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=2 peak_bytes=2264598616 capped=no load_ms=4804 first_token_ms=27900 tokens_per_s=1 tokens_in=324 tokens_out=29
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=3 peak_bytes=2267203672 capped=no load_ms=5382 first_token_ms=25550 tokens_per_s=6 tokens_in=324 tokens_out=29
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=4 peak_bytes=2264795224 capped=no load_ms=3418 first_token_ms=20646 tokens_per_s=4 tokens_in=324 tokens_out=29
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=5 peak_bytes=2263025752 capped=no load_ms=5389 first_token_ms=24198 tokens_per_s=3 tokens_in=324 tokens_out=29
timed model=minicpm5_2b machine=mac prompt=reply start=2026-11-11T21:46:30Z end=2026-11-11T23:00:50Z load1=353.19
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=1 peak_bytes=2215561256 capped=no load_ms=6283 first_token_ms=17419 tokens_per_s=0 tokens_in=186 tokens_out=512
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=2 peak_bytes=2216380432 capped=no load_ms=7859 first_token_ms=32595 tokens_per_s=0 tokens_in=186 tokens_out=512
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=3 peak_bytes=2217871376 capped=no load_ms=6343 first_token_ms=24702 tokens_per_s=0 tokens_in=186 tokens_out=512
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=4 peak_bytes=2213185528 capped=no load_ms=6375 first_token_ms=36575 tokens_per_s=1 tokens_in=186 tokens_out=512
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=5 peak_bytes=2213365776 capped=no load_ms=4218 first_token_ms=14547 tokens_per_s=2 tokens_in=186 tokens_out=512
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=action run=1 peak_bytes=2365566976 capped=no load_ms=2614 first_token_ms=6531 tokens_per_s=9 tokens_in=324 tokens_out=25
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=action run=2 peak_bytes=2365620224 capped=no load_ms=2438 first_token_ms=6522 tokens_per_s=9 tokens_in=324 tokens_out=25
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=action run=3 peak_bytes=2365579264 capped=no load_ms=2491 first_token_ms=6620 tokens_per_s=9 tokens_in=324 tokens_out=25
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=action run=4 peak_bytes=2365526016 capped=no load_ms=2430 first_token_ms=6642 tokens_per_s=9 tokens_in=324 tokens_out=25
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=action run=5 peak_bytes=2365628416 capped=no load_ms=2441 first_token_ms=6537 tokens_per_s=9 tokens_in=324 tokens_out=25
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=reply run=1 peak_bytes=2329559040 capped=no load_ms=2505 first_token_ms=3601 tokens_per_s=12 tokens_in=186 tokens_out=124
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=reply run=2 peak_bytes=2328846336 capped=no load_ms=2415 first_token_ms=3693 tokens_per_s=12 tokens_in=186 tokens_out=124
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=reply run=3 peak_bytes=2329624576 capped=no load_ms=2447 first_token_ms=3668 tokens_per_s=12 tokens_in=186 tokens_out=124
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=reply run=4 peak_bytes=2329620480 capped=no load_ms=2428 first_token_ms=3671 tokens_per_s=12 tokens_in=186 tokens_out=124
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=hosted-win prompt=reply run=5 peak_bytes=2329096192 capped=no load_ms=2392 first_token_ms=3668 tokens_per_s=12 tokens_in=186 tokens_out=124
timed model=granite_4_2_3b machine=mac prompt=action start=2026-11-11T23:00:50Z end=2026-11-11T23:04:46Z load1=134.32
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=1 peak_bytes=3268971328 capped=no load_ms=6163 first_token_ms=34598 tokens_per_s=0 tokens_in=340 tokens_out=32
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=2 peak_bytes=3268004648 capped=no load_ms=9124 first_token_ms=38961 tokens_per_s=1 tokens_in=340 tokens_out=32
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=3 peak_bytes=3266399040 capped=no load_ms=7017 first_token_ms=18406 tokens_per_s=7 tokens_in=340 tokens_out=32
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=4 peak_bytes=3263171392 capped=no load_ms=4804 first_token_ms=18248 tokens_per_s=16 tokens_in=340 tokens_out=32
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=5 peak_bytes=3265104704 capped=no load_ms=4041 first_token_ms=10784 tokens_per_s=15 tokens_in=340 tokens_out=32
timed model=granite_4_2_3b machine=mac prompt=reply start=2026-11-11T23:04:46Z end=2026-11-11T23:05:46Z load1=236.41
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=1 peak_bytes=3218606840 capped=no load_ms=4150 first_token_ms=7759 tokens_per_s=14 tokens_in=186 tokens_out=25
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=2 peak_bytes=3216263928 capped=no load_ms=3820 first_token_ms=5516 tokens_per_s=13 tokens_in=186 tokens_out=25
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=3 peak_bytes=3214822136 capped=no load_ms=3129 first_token_ms=6403 tokens_per_s=24 tokens_in=186 tokens_out=25
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=4 peak_bytes=3216362232 capped=no load_ms=2821 first_token_ms=3064 tokens_per_s=26 tokens_in=186 tokens_out=25
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=5 peak_bytes=3217918712 capped=no load_ms=2932 first_token_ms=3910 tokens_per_s=24 tokens_in=186 tokens_out=25
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=action run=1 peak_bytes=3300417536 capped=no load_ms=3735 first_token_ms=10265 tokens_per_s=6 tokens_in=340 tokens_out=31
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=action run=2 peak_bytes=3300397056 capped=no load_ms=3446 first_token_ms=10336 tokens_per_s=7 tokens_in=340 tokens_out=31
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=action run=3 peak_bytes=3300569088 capped=no load_ms=3468 first_token_ms=10375 tokens_per_s=7 tokens_in=340 tokens_out=31
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=action run=4 peak_bytes=3300331520 capped=no load_ms=3476 first_token_ms=10440 tokens_per_s=7 tokens_in=340 tokens_out=31
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=action run=5 peak_bytes=3300614144 capped=no load_ms=3494 first_token_ms=10489 tokens_per_s=7 tokens_in=340 tokens_out=31
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=reply run=1 peak_bytes=3268448256 capped=no load_ms=3525 first_token_ms=5653 tokens_per_s=8 tokens_in=186 tokens_out=23
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=reply run=2 peak_bytes=3268386816 capped=no load_ms=3448 first_token_ms=5651 tokens_per_s=8 tokens_in=186 tokens_out=23
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=reply run=3 peak_bytes=3268407296 capped=no load_ms=3479 first_token_ms=5756 tokens_per_s=8 tokens_in=186 tokens_out=23
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=reply run=4 peak_bytes=3268509696 capped=no load_ms=3467 first_token_ms=5688 tokens_per_s=8 tokens_in=186 tokens_out=23
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=hosted-win prompt=reply run=5 peak_bytes=3268427776 capped=no load_ms=3439 first_token_ms=5575 tokens_per_s=8 tokens_in=186 tokens_out=23
timed model=ministral_3_3b machine=mac prompt=action start=2026-11-11T23:05:46Z end=2026-11-11T23:07:02Z load1=208.32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=1 peak_bytes=3862105328 capped=no load_ms=3792 first_token_ms=5424 tokens_per_s=15 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=2 peak_bytes=3859549424 capped=no load_ms=3151 first_token_ms=5787 tokens_per_s=20 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=3 peak_bytes=3865726192 capped=no load_ms=2962 first_token_ms=4201 tokens_per_s=18 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=4 peak_bytes=3863317744 capped=no load_ms=3248 first_token_ms=9842 tokens_per_s=6 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=5 peak_bytes=3864988888 capped=no load_ms=4477 first_token_ms=11895 tokens_per_s=11 tokens_in=348 tokens_out=32
timed model=ministral_3_3b machine=mac prompt=reply start=2026-11-11T23:07:02Z end=2026-11-11T23:07:53Z load1=124.83
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=1 peak_bytes=3811429544 capped=no load_ms=3623 first_token_ms=3748 tokens_per_s=18 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=2 peak_bytes=3809365160 capped=no load_ms=3166 first_token_ms=5196 tokens_per_s=21 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=3 peak_bytes=3809103016 capped=no load_ms=3566 first_token_ms=5269 tokens_per_s=14 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=4 peak_bytes=3808644240 capped=no load_ms=3060 first_token_ms=2430 tokens_per_s=21 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=5 peak_bytes=3807431824 capped=no load_ms=3688 first_token_ms=2580 tokens_per_s=20 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=action run=1 peak_bytes=3479769088 capped=no load_ms=3901 first_token_ms=9864 tokens_per_s=6 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=action run=2 peak_bytes=3479801856 capped=no load_ms=3742 first_token_ms=10057 tokens_per_s=6 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=action run=3 peak_bytes=3479900160 capped=no load_ms=3738 first_token_ms=10032 tokens_per_s=6 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=action run=4 peak_bytes=3479703552 capped=no load_ms=3798 first_token_ms=9941 tokens_per_s=6 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=action run=5 peak_bytes=3479736320 capped=no load_ms=3778 first_token_ms=9934 tokens_per_s=6 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=reply run=1 peak_bytes=3443494912 capped=no load_ms=3770 first_token_ms=5176 tokens_per_s=8 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=reply run=2 peak_bytes=3443290112 capped=no load_ms=3729 first_token_ms=5042 tokens_per_s=8 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=reply run=3 peak_bytes=3443220480 capped=no load_ms=3756 first_token_ms=5341 tokens_per_s=8 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=reply run=4 peak_bytes=3442802688 capped=no load_ms=3776 first_token_ms=5071 tokens_per_s=8 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=hosted-win prompt=reply run=5 peak_bytes=3442995200 capped=no load_ms=3719 first_token_ms=5090 tokens_per_s=8 tokens_in=177 tokens_out=22
timed model=s5_nano_4b machine=mac prompt=action start=2026-11-11T23:07:53Z end=2026-11-11T23:09:36Z load1=166.51
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=1 peak_bytes=4318154256 capped=no load_ms=3970 first_token_ms=8556 tokens_per_s=11 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=2 peak_bytes=4326510096 capped=no load_ms=4359 first_token_ms=13296 tokens_per_s=8 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=3 peak_bytes=4314549800 capped=no load_ms=4190 first_token_ms=10471 tokens_per_s=7 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=4 peak_bytes=4318662208 capped=no load_ms=6456 first_token_ms=10414 tokens_per_s=8 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=5 peak_bytes=4327886376 capped=no load_ms=5785 first_token_ms=10580 tokens_per_s=8 tokens_in=349 tokens_out=25
timed model=s5_nano_4b machine=mac prompt=reply start=2026-11-11T23:09:36Z end=2026-11-11T23:11:57Z load1=162.22
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=1 peak_bytes=4246195680 capped=no load_ms=6161 first_token_ms=8231 tokens_per_s=15 tokens_in=182 tokens_out=63
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=2 peak_bytes=4247064056 capped=no load_ms=4875 first_token_ms=14230 tokens_per_s=7 tokens_in=182 tokens_out=63
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=3 peak_bytes=4244000224 capped=no load_ms=5668 first_token_ms=14856 tokens_per_s=7 tokens_in=182 tokens_out=63
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=4 peak_bytes=4246982112 capped=no load_ms=6326 first_token_ms=10647 tokens_per_s=11 tokens_in=182 tokens_out=63
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=5 peak_bytes=4243885536 capped=no load_ms=7157 first_token_ms=12894 tokens_per_s=5 tokens_in=182 tokens_out=63
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=action run=1 peak_bytes=4181131264 capped=no load_ms=4936 first_token_ms=13195 tokens_per_s=4 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=action run=2 peak_bytes=4180844544 capped=no load_ms=4361 first_token_ms=12802 tokens_per_s=4 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=action run=3 peak_bytes=4180791296 capped=no load_ms=4322 first_token_ms=13185 tokens_per_s=4 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=action run=4 peak_bytes=4181004288 capped=no load_ms=4428 first_token_ms=13242 tokens_per_s=4 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=action run=5 peak_bytes=4180844544 capped=no load_ms=4426 first_token_ms=13155 tokens_per_s=4 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=reply run=1 peak_bytes=4116443136 capped=no load_ms=4371 first_token_ms=6973 tokens_per_s=6 tokens_in=182 tokens_out=71
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=reply run=2 peak_bytes=4115238912 capped=no load_ms=4358 first_token_ms=6963 tokens_per_s=6 tokens_in=182 tokens_out=71
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=reply run=3 peak_bytes=4114464768 capped=no load_ms=4363 first_token_ms=7106 tokens_per_s=6 tokens_in=182 tokens_out=71
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=reply run=4 peak_bytes=4116008960 capped=no load_ms=4382 first_token_ms=7182 tokens_per_s=6 tokens_in=182 tokens_out=71
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=hosted-win prompt=reply run=5 peak_bytes=4115206144 capped=no load_ms=4443 first_token_ms=7186 tokens_per_s=5 tokens_in=182 tokens_out=71
timed model=gemma_4_e2b machine=mac prompt=action start=2026-11-11T23:11:57Z end=2026-11-11T23:14:14Z load1=215.84
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=1 peak_bytes=4444966536 capped=no load_ms=3478 first_token_ms=12082 tokens_per_s=3 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=2 peak_bytes=4444688008 capped=no load_ms=3537 first_token_ms=6386 tokens_per_s=8 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=3 peak_bytes=4446244488 capped=no load_ms=2185 first_token_ms=20046 tokens_per_s=5 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=4 peak_bytes=4445261424 capped=no load_ms=2763 first_token_ms=18680 tokens_per_s=8 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=5 peak_bytes=4443459208 capped=no load_ms=2976 first_token_ms=14798 tokens_per_s=12 tokens_in=363 tokens_out=32
timed model=gemma_4_e2b machine=mac prompt=reply start=2026-11-11T23:14:14Z end=2026-11-11T23:15:19Z load1=126.87
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=1 peak_bytes=4355296784 capped=no load_ms=2391 first_token_ms=6481 tokens_per_s=14 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=2 peak_bytes=4359065128 capped=no load_ms=2966 first_token_ms=6592 tokens_per_s=18 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=3 peak_bytes=4358426152 capped=no load_ms=3241 first_token_ms=5424 tokens_per_s=24 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=4 peak_bytes=4356836928 capped=no load_ms=1829 first_token_ms=1280 tokens_per_s=36 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=5 peak_bytes=4360195648 capped=no load_ms=1910 first_token_ms=11251 tokens_per_s=2 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=action run=1 peak_bytes=4679860224 capped=no load_ms=4028 first_token_ms=10523 tokens_per_s=6 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=action run=2 peak_bytes=4679843840 capped=no load_ms=3591 first_token_ms=10475 tokens_per_s=7 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=action run=3 peak_bytes=4679802880 capped=no load_ms=3574 first_token_ms=10540 tokens_per_s=7 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=action run=4 peak_bytes=4679606272 capped=no load_ms=3600 first_token_ms=10386 tokens_per_s=7 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=action run=5 peak_bytes=4679716864 capped=no load_ms=3572 first_token_ms=10328 tokens_per_s=6 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=reply run=1 peak_bytes=4607344640 capped=no load_ms=3661 first_token_ms=5063 tokens_per_s=11 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=reply run=2 peak_bytes=4607578112 capped=no load_ms=3618 first_token_ms=5107 tokens_per_s=12 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=reply run=3 peak_bytes=4607537152 capped=no load_ms=3709 first_token_ms=5146 tokens_per_s=11 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=reply run=4 peak_bytes=4607426560 capped=no load_ms=3608 first_token_ms=5129 tokens_per_s=13 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=hosted-win prompt=reply run=5 peak_bytes=4607578112 capped=no load_ms=3592 first_token_ms=5099 tokens_per_s=11 tokens_in=176 tokens_out=5
timed model=gemma_4_e4b machine=mac prompt=action start=2026-11-11T23:15:19Z end=2026-11-11T23:16:48Z load1=199.73
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=1 peak_bytes=7007507856 capped=no load_ms=2911 first_token_ms=11070 tokens_per_s=8 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=2 peak_bytes=7016895912 capped=no load_ms=3329 first_token_ms=8270 tokens_per_s=9 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=3 peak_bytes=7017354640 capped=no load_ms=3105 first_token_ms=5003 tokens_per_s=10 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=4 peak_bytes=7016093048 capped=no load_ms=2225 first_token_ms=4911 tokens_per_s=13 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=5 peak_bytes=7008212368 capped=no load_ms=2377 first_token_ms=5118 tokens_per_s=14 tokens_in=363 tokens_out=31
timed model=gemma_4_e4b machine=mac prompt=reply start=2026-11-11T23:16:48Z end=2026-11-12T00:31:57Z load1=82.20
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=1 peak_bytes=6919050592 capped=no load_ms=3414 first_token_ms=5794 tokens_per_s=9 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=2 peak_bytes=6918919520 capped=no load_ms=3605 first_token_ms=12904 tokens_per_s=1 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=3 peak_bytes=6924309832 capped=no load_ms=6312 first_token_ms=5937 tokens_per_s=0 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=4 peak_bytes=6918755704 capped=no load_ms=10433 first_token_ms=20723 tokens_per_s=0 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=5 peak_bytes=6923490680 capped=no load_ms=10310 first_token_ms=20096 tokens_per_s=0 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=action run=1 peak_bytes=7060672512 capped=no load_ms=5989 first_token_ms=21442 tokens_per_s=4 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=action run=2 peak_bytes=7060377600 capped=no load_ms=5528 first_token_ms=21304 tokens_per_s=4 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=action run=3 peak_bytes=7060627456 capped=no load_ms=5500 first_token_ms=21188 tokens_per_s=4 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=action run=4 peak_bytes=7060905984 capped=no load_ms=5546 first_token_ms=21192 tokens_per_s=4 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=action run=5 peak_bytes=7060688896 capped=no load_ms=5444 first_token_ms=20891 tokens_per_s=4 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=reply run=1 peak_bytes=6989119488 capped=no load_ms=5494 first_token_ms=10019 tokens_per_s=6 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=reply run=2 peak_bytes=6989352960 capped=no load_ms=5463 first_token_ms=10162 tokens_per_s=5 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=reply run=3 peak_bytes=6987575296 capped=no load_ms=5498 first_token_ms=10076 tokens_per_s=6 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=reply run=4 peak_bytes=6989320192 capped=no load_ms=5452 first_token_ms=10057 tokens_per_s=1 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=hosted-win prompt=reply run=5 peak_bytes=6989373440 capped=no load_ms=5339 first_token_ms=10364 tokens_per_s=5 tokens_in=176 tokens_out=512
bound model=minicpm5_2b machine=mac bound_bytes=2835349504 fits=yes
bound model=minicpm5_2b machine=hosted-win bound_bytes=2961178624 fits=yes
bound model=granite_4_2_3b machine=mac bound_bytes=4085252096 fits=yes
bound model=granite_4_2_3b machine=hosted-win bound_bytes=4127195136 fits=yes
bound model=ministral_3_3b machine=mac bound_bytes=4831838208 fits=yes
bound model=ministral_3_3b machine=hosted-win bound_bytes=4353687552 fits=yes
bound model=s5_nano_4b machine=mac bound_bytes=5402263552 fits=yes
bound model=s5_nano_4b machine=hosted-win bound_bytes=5226102784 fits=yes
bound model=gemma_4_e2b machine=mac bound_bytes=5561647104 fits=yes
bound model=gemma_4_e2b machine=hosted-win bound_bytes=5855248384 fits=yes
bound model=gemma_4_e4b machine=mac bound_bytes=8774483968 fits=no
bound model=gemma_4_e4b machine=hosted-win bound_bytes=8833204224 fits=no
verdict candidates=minicpm5_2b,granite_4_2_3b,ministral_3_3b
```
