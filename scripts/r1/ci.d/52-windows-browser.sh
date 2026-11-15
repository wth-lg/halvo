#!/bin/sh
# jobs: windows
# rule: the browser group of the Windows functional check (the page tests, the fixtures and the known defects) runs by the rules of 50-windows-functional in a script of its own, so that each of the two stays within the driver's limit of seconds
set -u
WF_GROUP=browser
export WF_GROUP
exec sh "$(dirname "$0")/50-windows-functional.sh" "$@"
