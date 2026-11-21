# Changelog entries

A pull request adds one new file here, named after its branch (`<branch>.md`), with
one line for each type of change that it makes: `- <Type>: <text>`, where Type is
Added, Changed, Deprecated, Removed, Fixed or Security. It does not change
`CHANGELOG.md`, so two pull requests never change the same lines. A branch that
wrote lines under the Unreleased heading runs `git fetch origin`, then
`git merge origin/main`, then `sh scripts/changelog-entry.sh`, which moves the lines
there that `origin/main` lacks into the file of the branch. At a release,
`sh scripts/changelog-join.sh` joins the files into `CHANGELOG.md` and removes them.
`sh scripts/check-changelog.sh` checks both.
