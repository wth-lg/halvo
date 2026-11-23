#!/bin/sh
# jobs: windows
# rule: the long form of 52-windows-browser, part top (the rows at the top level of the test file): a run by hand alone (the workflow's input checks names this check, which stands in no slice and no schedule), it runs every row of the part, the rows marked scope=long-only among them, by the rules of 50-windows-functional and with the tests' rounds of the short form, and a row marked scope=long-only-shows-sometimes that reads 0 is printed with result=not-shown and fails nothing
# lane: alone
set -u
WF_GROUP=browser
WF_FORM=long
WF_PART=top
export WF_GROUP WF_FORM WF_PART
exec sh "$(dirname "$0")/50-windows-functional.sh" "$@"
