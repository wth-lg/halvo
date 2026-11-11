# The routing group in the model trials

This file records which of the four models of the routing work may enter the model trials as the group `routing`, as their publishers' pages on Hugging Face stated them on 2026-11-11 (UTC). A model enters only when its licence is MIT or Apache-2.0, its 4-bit GGUF file is in a repository of its publisher's own account, that file downloads with no account, and the publisher's file listing at a named commit states the file's size in bytes and its SHA-256.

S5-nano (`s5_nano_4b`), Gemma 4 E4B (`gemma_4_e4b`) and Gemma 4 E2B (`gemma_4_e2b`) enter. Each card names Apache 2.0, each repository is open with no sign-in, and each listing states the size and the hash of the file. Ternary-Bonsai-2-9B (`ternary_bonsai_2_9b`) is left out. Its card names Apache 2.0, but its publisher's repository holds no file whose name holds `q4_k_m` or `q4_0`, so it has no 4-bit file of its publisher's own.

In the block, the `ask` line of a model gives its result, a `licence` line gives each licence that its card names, and a `weights` line gives the file of a model that enters, with its size, its hash and its address at the commit that was read. The sizes and hashes are the ones that the publishers' listings state, and the trial's fetch script checks them on the bytes that it downloads.

```
ask model=s5_nano_4b repo=https://huggingface.co/maritaca-ai/S5-nano-GGUF file=S5-nano-Q4_K_M.gguf own_file=yes account=no page_hash=yes licence=yes ok=yes
licence item=s5_nano_4b spdx=Apache-2.0 read=2026-11-11 ok=yes
weights model=s5_nano_4b group=routing file=S5-nano-Q4_K_M.gguf sha256=c7040c0770c427d06eb22643e41f061a1bc30e267f07dbcad5a9861b4564003b bytes=2783446688 url=https://huggingface.co/maritaca-ai/S5-nano-GGUF/resolve/892d3b6ce018802d74fed1650e712e5fcd5a42b5/S5-nano-Q4_K_M.gguf spdx=Apache-2.0
ask model=gemma_4_e4b repo=https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf file=gemma-4-E4B_q4_0-it.gguf own_file=yes account=no page_hash=yes licence=yes ok=yes
licence item=gemma_4_e4b spdx=Apache-2.0 read=2026-11-11 ok=yes
weights model=gemma_4_e4b group=routing file=gemma-4-E4B_q4_0-it.gguf sha256=676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee bytes=5154941280 url=https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf/resolve/4b4a2c1d584be7264f87aac328a1bc739ce81b6c/gemma-4-E4B_q4_0-it.gguf spdx=Apache-2.0
ask model=gemma_4_e2b repo=https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf file=gemma-4-E2B_q4_0-it.gguf own_file=yes account=no page_hash=yes licence=yes ok=yes
licence item=gemma_4_e2b spdx=Apache-2.0 read=2026-11-11 ok=yes
weights model=gemma_4_e2b group=routing file=gemma-4-E2B_q4_0-it.gguf sha256=fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634 bytes=3349516256 url=https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf/resolve/675cff42a74c774d6cb76f76d8eacb49b48c9b93/gemma-4-E2B_q4_0-it.gguf spdx=Apache-2.0
ask model=ternary_bonsai_2_9b repo=https://huggingface.co/prism-ml/Ternary-Bonsai-2-9B-gguf file=- own_file=no account=no page_hash=no licence=yes ok=no
licence item=ternary_bonsai_2_9b spdx=Apache-2.0 read=2026-11-11 ok=yes
```
