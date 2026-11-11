# Model cells (R3, unit U0.5)

Each model of `tools/r3-trials/model/weights.txt` was loaded by the trial loader `r3-model`
(llama.cpp of the crate llama-cpp-2 0.1.161 on the CPU, no layer on a GPU, the weights read into
memory, under the model lock of halvo-cap) and answered the two prompts of
`tools/r3-trials/model/prompts/` at the level `medium`, one request per run, under `measure-peak
--limit 8589934592 --runs 5` of `crates/halvo-cap` as main holds it. On `mac` (M3 Max, 64 GB) each
cell went through the timed-run helper, one at a time; its `timed` line gives the helper's start
and end and the 1-minute load when it started. On `hosted-win` (windows-2025) no cell has run yet: its scratch run is queued. `hosted-mac` (macos-15, 7 GB) is not used.
How the child works and how to run a cell: `tools/r3-trials/model/README.md`.

The budget is 8,589,934,592 - (1,216,348,160 + 8,388,608) - 268,435,456 = 7,096,762,368 bytes,
from `browser_bound_bytes=1216348160` of `docs/r3/trials/browser-profile.md`. `python3 -I
tools/r3-trials/model/fits.py --write` computes the `bound` and `verdict` lines from the `cell`
lines, again with no new run when that bound changes.

Three models of the contract left the list (their `notrun` lines): `qwen3_5_2b` and `qwen3_5_4b`,
because none of the Qwen organisation's 54 GGUF repositories holds Qwen3.5, and
`ternary_bonsai_2_9b`, because its publisher's repository holds 2-bit and ternary files and no
4-bit one. llama.cpp's template engine of llama-cpp-2 0.1.161 does not apply Gemma 4's chat
template, so the child sent the text of `gemma_4_e2b` and `gemma_4_e4b` as plain text: their
cells show the footprint and the speed of a request of that size, and their replies test nothing.

Choices that the contract leaves open, as built: 4 threads, a batch of 2,048 tokens and a
micro-batch of 512; the model's own chat template with a system turn and a user turn; the verdict
keeps the order of A13.5's table, which `weights.txt` keeps; `tokens_per_s` is `tokens_out` times
1,000 over the time from the first reply token to the end of the reply; a prompt with fewer than
five `cell` lines on a machine gives that machine `bound_bytes=0 fits=not-measured`.

What the lines cannot show. On `mac`, `check-binary.sh` of the workflow run's commit 896950c found
no socket call, no TLS, URL-loading or Metal symbol and no HTTP or download code in the release
binary of `r3-model`; on `hosted-win` no tool shows that no network code is linked into the loader.
`capped=no` says that no read was above the limit (on Windows, that no report of a refused
allocation arrived), not that the child held no more. Not counted on macOS: other processes (the
child starts none), memory before an exec (it does not exec), mapped pages (it maps no weights and
refuses a tensor above 4 GiB, which llama.cpp would map), purgeable and reusable memory, kernel
messages and socket buffers; on Windows: other processes, mapped files, sections and broker
processes. Every cell on `mac` started at a 1-minute load above 20, so its times and speeds are no
figure of a quiet machine; the footprint does not depend on the load.

```text
runtime crate=0.1.161 backend=cpu load=none sampling=greedy ctx=8192 batch=2048 threads=4 cache=f16
licence item=qwen3_5_2b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=minicpm5_2b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=granite_4_2_3b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=ministral_3_3b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=qwen3_5_4b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=s5_nano_4b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=gemma_4_e2b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=gemma_4_e4b spdx=Apache-2.0 read=2026-11-11 ok=yes
licence item=ternary_bonsai_2_9b spdx=Apache-2.0 read=2026-11-11 ok=yes
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
timed model=minicpm5_2b machine=mac prompt=action start=2026-11-11T20:45:19Z end=2026-11-11T20:46:01Z load1=159.40
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=1 peak_bytes=2262534232 capped=no load_ms=1837 first_token_ms=3413 tokens_per_s=23 tokens_in=324 tokens_out=29
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=2 peak_bytes=2263746648 capped=no load_ms=1826 first_token_ms=2937 tokens_per_s=25 tokens_in=324 tokens_out=29
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=3 peak_bytes=2266204248 capped=no load_ms=2072 first_token_ms=4191 tokens_per_s=20 tokens_in=324 tokens_out=29
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=4 peak_bytes=2267023448 capped=no load_ms=2039 first_token_ms=4452 tokens_per_s=23 tokens_in=324 tokens_out=29
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=action run=5 peak_bytes=2264729712 capped=no load_ms=2188 first_token_ms=4873 tokens_per_s=19 tokens_in=324 tokens_out=29
timed model=minicpm5_2b machine=mac prompt=reply start=2026-11-11T20:46:01Z end=2026-11-11T20:47:48Z load1=98.95
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=1 peak_bytes=2213464080 capped=no load_ms=1833 first_token_ms=1582 tokens_per_s=34 tokens_in=186 tokens_out=512
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=2 peak_bytes=2215004176 capped=no load_ms=1806 first_token_ms=1525 tokens_per_s=30 tokens_in=186 tokens_out=512
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=3 peak_bytes=2213496848 capped=no load_ms=1859 first_token_ms=2479 tokens_per_s=26 tokens_in=186 tokens_out=512
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=4 peak_bytes=2215315496 capped=no load_ms=1909 first_token_ms=1624 tokens_per_s=31 tokens_in=186 tokens_out=512
cell model=minicpm5_2b file_sha256=ec2d5801640099e97d8d7e8003ad4d81f336e757811f03a26173dddf386602fd machine=mac prompt=reply run=5 peak_bytes=2218330128 capped=no load_ms=1917 first_token_ms=1654 tokens_per_s=32 tokens_in=186 tokens_out=512
notrun model=minicpm5_2b machine=hosted-win prompt=action cause=not_taken
notrun model=minicpm5_2b machine=hosted-win prompt=reply cause=not_taken
timed model=granite_4_2_3b machine=mac prompt=action start=2026-11-11T20:47:48Z end=2026-11-11T20:48:39Z load1=52.48
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=1 peak_bytes=3264039768 capped=no load_ms=2615 first_token_ms=4395 tokens_per_s=20 tokens_in=340 tokens_out=32
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=2 peak_bytes=3265841984 capped=no load_ms=2623 first_token_ms=4440 tokens_per_s=20 tokens_in=340 tokens_out=32
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=3 peak_bytes=3263220544 capped=no load_ms=2611 first_token_ms=4710 tokens_per_s=19 tokens_in=340 tokens_out=32
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=4 peak_bytes=3267824472 capped=no load_ms=2662 first_token_ms=4299 tokens_per_s=20 tokens_in=340 tokens_out=32
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=action run=5 peak_bytes=3264990016 capped=no load_ms=2635 first_token_ms=4334 tokens_per_s=20 tokens_in=340 tokens_out=32
timed model=granite_4_2_3b machine=mac prompt=reply start=2026-11-11T20:48:39Z end=2026-11-11T20:49:16Z load1=35.80
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=1 peak_bytes=3215084304 capped=no load_ms=2624 first_token_ms=2636 tokens_per_s=25 tokens_in=186 tokens_out=25
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=2 peak_bytes=3214248696 capped=no load_ms=2617 first_token_ms=2581 tokens_per_s=26 tokens_in=186 tokens_out=25
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=3 peak_bytes=3219082000 capped=no load_ms=2728 first_token_ms=2436 tokens_per_s=27 tokens_in=186 tokens_out=25
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=4 peak_bytes=3214297824 capped=no load_ms=2641 first_token_ms=2462 tokens_per_s=26 tokens_in=186 tokens_out=25
cell model=granite_4_2_3b file_sha256=e0406663965846ae22a403456eb826ccce5f450840491f71952f18a7cb78e7d5 machine=mac prompt=reply run=5 peak_bytes=3214363360 capped=no load_ms=2597 first_token_ms=2440 tokens_per_s=27 tokens_in=186 tokens_out=25
notrun model=granite_4_2_3b machine=hosted-win prompt=action cause=not_taken
notrun model=granite_4_2_3b machine=hosted-win prompt=reply cause=not_taken
timed model=ministral_3_3b machine=mac prompt=action start=2026-11-11T20:58:51Z end=2026-11-11T21:01:44Z load1=45.27
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=1 peak_bytes=3871771888 capped=no load_ms=4810 first_token_ms=12243 tokens_per_s=13 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=2 peak_bytes=3868953840 capped=no load_ms=7575 first_token_ms=18012 tokens_per_s=9 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=3 peak_bytes=3867806960 capped=no load_ms=7379 first_token_ms=18531 tokens_per_s=11 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=4 peak_bytes=3866578112 capped=no load_ms=4236 first_token_ms=22882 tokens_per_s=2 tokens_in=348 tokens_out=32
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=action run=5 peak_bytes=3866299584 capped=no load_ms=7305 first_token_ms=25456 tokens_per_s=2 tokens_in=348 tokens_out=32
timed model=ministral_3_3b machine=mac prompt=reply start=2026-11-11T20:33:32Z end=2026-11-11T20:34:40Z load1=186.41
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=1 peak_bytes=3814804696 capped=no load_ms=3498 first_token_ms=4566 tokens_per_s=18 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=2 peak_bytes=3812494504 capped=no load_ms=4156 first_token_ms=5946 tokens_per_s=19 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=3 peak_bytes=3811691664 capped=no load_ms=3732 first_token_ms=9319 tokens_per_s=12 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=4 peak_bytes=3813706920 capped=no load_ms=3175 first_token_ms=6068 tokens_per_s=17 tokens_in=177 tokens_out=22
cell model=ministral_3_3b file_sha256=9ed150d4367e68df0ac8e1540f6ddc65b42d0ee26378329d1ecbca60f93fc5f8 machine=mac prompt=reply run=5 peak_bytes=3812281512 capped=no load_ms=4535 first_token_ms=7511 tokens_per_s=13 tokens_in=177 tokens_out=22
notrun model=ministral_3_3b machine=hosted-win prompt=action cause=not_taken
notrun model=ministral_3_3b machine=hosted-win prompt=reply cause=not_taken
timed model=s5_nano_4b machine=mac prompt=action start=2026-11-11T20:49:16Z end=2026-11-11T20:50:24Z load1=34.57
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=1 peak_bytes=4323888680 capped=no load_ms=3661 first_token_ms=8239 tokens_per_s=13 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=2 peak_bytes=4315483688 capped=no load_ms=3684 first_token_ms=5648 tokens_per_s=15 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=3 peak_bytes=4322561528 capped=no load_ms=3568 first_token_ms=6358 tokens_per_s=15 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=4 peak_bytes=4323364368 capped=no load_ms=3629 first_token_ms=6219 tokens_per_s=14 tokens_in=349 tokens_out=25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=action run=5 peak_bytes=4314517032 capped=no load_ms=3772 first_token_ms=5702 tokens_per_s=14 tokens_in=349 tokens_out=25
timed model=s5_nano_4b machine=mac prompt=reply start=2026-11-11T20:50:24Z end=2026-11-11T20:51:58Z load1=28.25
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=1 peak_bytes=4244245984 capped=no load_ms=3670 first_token_ms=6100 tokens_per_s=14 tokens_in=182 tokens_out=63
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=2 peak_bytes=4244311520 capped=no load_ms=5700 first_token_ms=5573 tokens_per_s=14 tokens_in=182 tokens_out=63
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=3 peak_bytes=4246080968 capped=no load_ms=5121 first_token_ms=7789 tokens_per_s=13 tokens_in=182 tokens_out=63
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=4 peak_bytes=4245818848 capped=no load_ms=6465 first_token_ms=9138 tokens_per_s=12 tokens_in=182 tokens_out=63
cell model=s5_nano_4b file_sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b machine=mac prompt=reply run=5 peak_bytes=4242755040 capped=no load_ms=5122 first_token_ms=7140 tokens_per_s=16 tokens_in=182 tokens_out=63
notrun model=s5_nano_4b machine=hosted-win prompt=action cause=not_taken
notrun model=s5_nano_4b machine=hosted-win prompt=reply cause=not_taken
timed model=gemma_4_e2b machine=mac prompt=action start=2026-11-11T20:51:58Z end=2026-11-11T20:53:02Z load1=42.21
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=1 peak_bytes=4449259120 capped=no load_ms=2003 first_token_ms=7610 tokens_per_s=12 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=2 peak_bytes=4452159112 capped=no load_ms=2184 first_token_ms=7754 tokens_per_s=18 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=3 peak_bytes=4451536496 capped=no load_ms=1575 first_token_ms=3871 tokens_per_s=14 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=4 peak_bytes=4448636552 capped=no load_ms=1720 first_token_ms=5644 tokens_per_s=16 tokens_in=363 tokens_out=32
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=action run=5 peak_bytes=4456451696 capped=no load_ms=1816 first_token_ms=6088 tokens_per_s=15 tokens_in=363 tokens_out=32
timed model=gemma_4_e2b machine=mac prompt=reply start=2026-11-11T20:53:02Z end=2026-11-11T20:53:37Z load1=43.68
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=1 peak_bytes=4359835176 capped=no load_ms=1600 first_token_ms=1884 tokens_per_s=31 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=2 peak_bytes=4360457792 capped=no load_ms=1642 first_token_ms=1859 tokens_per_s=14 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=3 peak_bytes=4362391104 capped=no load_ms=1698 first_token_ms=2879 tokens_per_s=17 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=4 peak_bytes=4360048168 capped=no load_ms=1677 first_token_ms=2795 tokens_per_s=18 tokens_in=176 tokens_out=5
cell model=gemma_4_e2b file_sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 machine=mac prompt=reply run=5 peak_bytes=4359867944 capped=no load_ms=1749 first_token_ms=2462 tokens_per_s=27 tokens_in=176 tokens_out=5
notrun model=gemma_4_e2b machine=hosted-win prompt=action cause=not_taken
notrun model=gemma_4_e2b machine=hosted-win prompt=reply cause=not_taken
timed model=gemma_4_e4b machine=mac prompt=action start=2026-11-11T20:53:37Z end=2026-11-11T20:55:00Z load1=40.60
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=1 peak_bytes=7012062608 capped=no load_ms=3539 first_token_ms=7884 tokens_per_s=11 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=2 peak_bytes=7008179576 capped=no load_ms=2498 first_token_ms=7716 tokens_per_s=12 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=3 peak_bytes=7018845584 capped=no load_ms=2571 first_token_ms=7427 tokens_per_s=13 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=4 peak_bytes=7013995920 capped=no load_ms=2673 first_token_ms=5956 tokens_per_s=12 tokens_in=363 tokens_out=31
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=action run=5 peak_bytes=7018632592 capped=no load_ms=2574 first_token_ms=5191 tokens_per_s=11 tokens_in=363 tokens_out=31
timed model=gemma_4_e4b machine=mac prompt=reply start=2026-11-11T20:55:01Z end=2026-11-11T20:58:45Z load1=42.87
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=1 peak_bytes=6923785568 capped=no load_ms=2348 first_token_ms=3723 tokens_per_s=18 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=2 peak_bytes=6919509368 capped=no load_ms=2591 first_token_ms=3324 tokens_per_s=17 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=3 peak_bytes=6923867512 capped=no load_ms=2728 first_token_ms=4197 tokens_per_s=19 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=4 peak_bytes=6926718328 capped=no load_ms=2434 first_token_ms=4621 tokens_per_s=15 tokens_in=176 tokens_out=512
cell model=gemma_4_e4b file_sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee machine=mac prompt=reply run=5 peak_bytes=6922032480 capped=no load_ms=4212 first_token_ms=12355 tokens_per_s=10 tokens_in=176 tokens_out=512
notrun model=gemma_4_e4b machine=hosted-win prompt=action cause=not_taken
notrun model=gemma_4_e4b machine=hosted-win prompt=reply cause=not_taken
bound model=minicpm5_2b machine=mac bound_bytes=2835349504 fits=yes
bound model=minicpm5_2b machine=hosted-win bound_bytes=0 fits=not-measured
bound model=granite_4_2_3b machine=mac bound_bytes=4085252096 fits=yes
bound model=granite_4_2_3b machine=hosted-win bound_bytes=0 fits=not-measured
bound model=ministral_3_3b machine=mac bound_bytes=4840226816 fits=yes
bound model=ministral_3_3b machine=hosted-win bound_bytes=0 fits=not-measured
bound model=s5_nano_4b machine=mac bound_bytes=5410652160 fits=yes
bound model=s5_nano_4b machine=hosted-win bound_bytes=0 fits=not-measured
bound model=gemma_4_e2b machine=mac bound_bytes=5570035712 fits=yes
bound model=gemma_4_e2b machine=hosted-win bound_bytes=0 fits=not-measured
bound model=gemma_4_e4b machine=mac bound_bytes=8774483968 fits=no
bound model=gemma_4_e4b machine=hosted-win bound_bytes=0 fits=not-measured
verdict candidates=-
```
