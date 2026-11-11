# The coordinator's models

This directory holds two TLA+ models of the coordinator that the crate `halvo-coord` will
implement, and the traces that the crate's tests replay.

## The checker

The models are checked by TLC, which runs from `tla2tools.jar` of the tlaplus project's
release v1.7.4. On 2026-11-11 the address
`https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar` led to that
release. GitHub gives that address to the newest release that is neither a draft nor a
pre-release, so a pre-release is never the pinned one. The jar is not in this repository.
Every command here takes its path with `--jar`.

```
jar url=https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar bytes=2274532 sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
```

The licence, read from the jar itself: `License.txt` is the MIT License, with the line
"Copyright (c) 2019 Microsoft Research". The jar also holds the classes `javax/mail`,
`javax/activation` and `com/sun/mail`, and `META-INF/LICENSE.txt` is the text of the
Eclipse Public License 2.0. Nothing in this repository links the jar or holds a copy of it.
