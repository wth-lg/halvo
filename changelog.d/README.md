# Changelog entries

A pull request adds one new file here, named after its branch (`<branch>.md`), with
one line for each type of change that it makes: `- <Type>: <text>`, where Type is
Added, Changed, Deprecated, Removed, Fixed or Security. It leaves `CHANGELOG.md`
from its Unreleased heading down as it is, so two pull requests never change the
same lines. `sh scripts/changelog-entry.sh` moves the lines that a branch wrote under
that heading into the file of the branch. At a release, `sh scripts/changelog-join.sh`
joins the files into `CHANGELOG.md` and removes them. `sh scripts/check-changelog.sh`
checks both.
