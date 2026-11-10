# Halvo

Halvo is a desktop app for macOS and Windows that lets a person with no technical
background get work done through AI agents. This repository holds the Tauri 2 app in
`src-tauri/`, the capture crate in `crates/`, and the scripts and CI workflow that
build, check and release them.

## Rules

The rule table of this repository is below, and `CLAUDE.md` imports this file.

| Rule | Check | What the error tells you to do |
|---|---|---|
| A commit message has no `Co-Authored-By:` trailer and no "Generated with" line, because the configured git user is the only author of a commit. | `scripts/check-commit-messages.sh`. `.githooks/commit-msg` runs it on `git commit` and `git merge` in a clone where `sh scripts/prerequisites.sh` has set `core.hooksPath`. The `Commit messages` step of the macOS CI job runs it on the commits of every push and pull request. It exempts the commits reachable from `685d846`, because on 2026-11-09 the owner chose to keep the eight commits that already carried attribution lines. | It prints each offending commit and line, and how to remove the line. |
| Two branches that each add lines to `CHANGELOG.md` merge without a conflict in that file. | `.gitattributes` gives `CHANGELOG.md` git's union merge, which keeps the lines of both sides. | There is no error. If the file still conflicts, your branch was cut before `.gitattributes` existed. Run `git merge --abort`, then `git checkout origin/main -- .gitattributes`, commit, and merge again. |
