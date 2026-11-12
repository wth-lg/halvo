# Fixtures of the license check

Each directory here and each file whose name ends in `.json` is one fixture of
`sh gate/checks/85-licenses.sh --self-test`. This file is neither. The self-test compares the
number of fixtures that ran with the number of fixtures here, and with this file here it fails
when it counts an entry that is no fixture.
