#!/bin/sh
# jobs: macos
# rule: TLC finds none of the eleven invariants of the spend model's Spend.cfg violated, the machine of one hold gives every committed trace with all 30 pairs and 8 causes, the replay control accepts each of its right traces and refuses each of its wrong ones, and each of the two controls breaks its one invariant; the macos job alone runs it, because the model and the checker, one pinned jar, are the same on both systems and the windows job printed the same lines of T7.6 in run 43294110696; what the check holds and what it does not see stands in formal/r6-spend/README.md, section "The check 55-r6-formal.sh".
# lane: free
set -u
name=55-r6-formal
model=formal/r6-spend
inputs='README.md Hold.tla Spend.tla Spend.cfg ControlRuling.cfg ControlResend.cfg Replay.tla Replay.cfg traces.jsonl'
invariants='S1 S2 S3 S4 S5 S6 S7 S8 S9 S10 S11'
jar_url=https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
jar_bytes=2274532
jar_sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
fetch_connect=10
fetch_max_time=25
fetch_retries=1
fetch_retry_delay=5
tlc_banner='TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)'
config='CONSTANTS
Null = Null
Runs = {r1, r2}
MaxRequests = 2
MaxAdmissions = 2
MaxCapChanges = 2
MaxOtherSpends = 2
MaxLostAnswers = 1
MaxStops = 1
Hurry = TRUE
TraceMoves = 12
Fault = ?
SYMMETRY RunSymmetry
INIT Init
NEXT Next
CHECK_DEADLOCK FALSE'
replay_config='CONSTANTS
Null = Null'
right_traces='{"seed":1,"count":2}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":3,"active":"yes","spent":0,"held":1,"moves":[{"move":"reserve","arg":2,"answer":"ok","state":"held","spent":0,"held":3,"cost":0},{"move":"dispatch","arg":null,"answer":"ok","state":"sent","spent":0,"held":3,"cost":0},{"move":"settle","arg":1,"answer":"ok","state":"settled","spent":1,"held":1,"cost":1},{"move":"reserve","arg":2,"answer":"repeat","state":"settled","spent":1,"held":1,"cost":1},{"move":"settle","arg":2,"answer":"state","state":"settled","spent":1,"held":1,"cost":1}]}'
wrong_traces='{"seed":1,"count":26}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"state","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"sent","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":2,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":1},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"held","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":2,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":1,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":1},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"held","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"sent","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"held","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":2,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":2,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":2,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":2,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":1,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":1,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":1},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":1},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":1}]}
{"cap":2,"active":"yes","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}
{"cap":2,"active":"no","spent":1,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":0,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"held","spent":1,"held":1,"cost":0},{"move":"void","arg":null,"answer":"ok","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":3,"answer":"other_arguments","state":"voided","spent":1,"held":0,"cost":0},{"move":"reserve","arg":1,"answer":"repeat","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"expire","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"settle","arg":0,"answer":"state","state":"voided","spent":1,"held":0,"cost":0},{"move":"dispatch","arg":null,"answer":"used","state":"voided","spent":1,"held":0,"cost":0},{"move":"void","arg":null,"answer":"state","state":"voided","spent":1,"held":0,"cost":0}]}'
right_count=2
wrong_count=26
cause=
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

setup_failed() {
  printf 'FAIL %s (setup)\n' "$name"
  exit 1
}

first() {
  [ -n "$cause" ] || cause=$1
}

refuse() {
  first "$1"
  printf '%s\n%s\n' "$2" "$3" >"$artifacts/refused-$2.txt"
}

enter_repo() {
  case ${TMPDIR:-/tmp} in
    /*) ;;
    *) setup_failed ;;
  esac
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || setup_failed
  fi
  cd "$repo" || setup_failed
  repo=$(pwd -P) || setup_failed
  base=$(cd "${TMPDIR:-/tmp}" && pwd -P) || setup_failed
  case $base/ in
    "$repo"/*) setup_failed ;;
  esac
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || setup_failed
  artifacts=$(cd "$artifacts" && pwd -P) || setup_failed
  work=$(mktemp -d "$base/$name.XXXXXX") || setup_failed
  mkdir "$work/in" || setup_failed
  printf '%s\n' "$config" >"$work/config.want" || setup_failed
  printf '%s\n' "$replay_config" >"$work/replay-config.want" || setup_failed
  tree_state "$work/tree.before" || setup_failed
}

tree_state() {
  GIT_OPTIONAL_LOCKS=0 git -C "$repo" status --porcelain=v2 --branch --untracked-files=all >"$1" 2>&1
}

tree_kept() {
  if tree_state "$work/tree.$2" && cmp -s "$work/tree.before" "$work/tree.$2"; then
    return 0
  fi
  refuse "$1" "tree-$2" "HEAD, the index or the changed files of the repository differ after the step $2"
  return 1
}

read_inputs() {
  missing=
  for file in $inputs; do
    git cat-file blob "HEAD:$model/$file" >"$work/in/$file" 2>/dev/null || missing="$missing $file"
  done
}

present() {
  for file in "$@"; do
    case "$missing " in
      *" $file "*) return 1 ;;
    esac
  done
}

pin_kept() {
  present README.md || return 1
  pin=$(sed -n 's/^tlc url=\([^ ]*\) bytes=\([1-9][0-9]*\) sha256=\([0-9a-f]\{64\}\)$/\1 \2 \3/p' "$work/in/README.md")
  [ "$pin" = "$jar_url $jar_bytes $jar_sha256" ]
}

fetch_jar() {
  curl -q --proto =https --proto-redir =https -fsS -L --connect-timeout "$fetch_connect" --max-time "$2" \
    --retry "$3" --retry-delay "$4" -o "$5" --url "$1" >"$artifacts/curl.txt" 2>&1 </dev/null
}

jar_kept() {
  size=$(wc -c <"$1") || return 1
  size=$((size + 0))
  sum=$(shasum -a 256 <"$1") || return 1
  sum=${sum%% *}
  [ "$size" = "$jar_bytes" ] && [ "$sum" = "$jar_sha256" ]
}

find_java() {
  unset JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS
  if [ -n "${JAVA_HOME:-}" ]; then
    java=$JAVA_HOME/bin/java
  else
    java=java
  fi
  found=$(command -v "$java") || return 1
  case $found in
    /*) java=$found ;;
    *) java=$repo/$found ;;
  esac
  "$java" -version >"$artifacts/java.txt" 2>&1 </dev/null || return 1
  major=$(sed -n 's/.* version "\([0-9][0-9]*\).*/\1/p' "$artifacts/java.txt")
  major=${major%%"
"*}
  case $major in
    '' | *[!0-9]*) return 1 ;;
  esac
  [ "$major" -ge 17 ]
}

prepare() {
  if ! pin_kept; then
    refuse hash pin "the line tlc url=... of README.md does not name the address, size and SHA-256 that the script holds"
    return 1
  fi
  if ! fetch_jar "$jar_url" "$fetch_max_time" "$fetch_retries" "$fetch_retry_delay" "$work/tla2tools.jar"; then
    refuse fetch fetch "curl did not fetch the jar from the address that the script holds"
    return 1
  fi
  tree_kept fetch fetch || return 1
  if ! jar_kept "$work/tla2tools.jar"; then
    refuse hash hash "the fetched jar has another size or SHA-256 than the script holds"
    return 1
  fi
  if ! find_java; then
    refuse java java "no java of major version 17 or later at JAVA_HOME or on the path"
    return 1
  fi
}

count_workers() {
  workers=$(getconf _NPROCESSORS_ONLN 2>/dev/null) || workers=
  case $workers in
    '' | *[!0-9]* | 0*) workers=1 ;;
  esac
  [ "$workers" -le 4 ] || workers=4
}

read_config() {
  awk -v want="$3" '
    BEGIN {
      split("CONSTANT CONSTANTS INIT NEXT SPECIFICATION INVARIANT INVARIANTS PROPERTY PROPERTIES SYMMETRY VIEW CONSTRAINT CONSTRAINTS ACTION_CONSTRAINT ACTION_CONSTRAINTS CHECK_DEADLOCK POSTCONDITION ALIAS", list, " ")
      for (i in list) key[list[i]] = 1
      wanted = split(want, list, " ")
      for (i in list) wants[list[i]] = 1
      norm = ENVIRON["READ_CONFIG_NORM"]
      printf "" >norm
    }
    {
      line = $0
      if (index(line, "(*") || index(line, "*)") || index(line, "\r")) bad++
      at = index(line, "\\*")
      if (at) line = substr(line, 1, at - 1)
      n = split(line, word, /[ \t]+/)
      out = ""
      for (i = 1; i <= n; i++) {
        w = word[i]
        if (w == "") continue
        if (w in key) section = w
        if (section == "INVARIANT" || section == "INVARIANTS") {
          if (w in key) continue
          if (w in seen) bad++
          seen[w] = 1
          count++
          if (!(w in wants)) bad++
          continue
        }
        if (w == "Fault") {
          if (word[i + 1] == "=" && word[i + 2] ~ /^"[a-z_]+"$/) fault = word[i + 2]
          else bad++
          out = out " Fault = ?"
          i += 2
          continue
        }
        out = out " " w
      }
      if (out != "") print substr(out, 2) >norm
    }
    END {
      if (count + 0 != wanted) bad++
      printf "%d %s %d\n", count, (fault == "" ? "-" : fault), bad
    }' "$1"
}

config_kept() {
  READ_CONFIG_NORM=$work/$2.norm
  export READ_CONFIG_NORM
  read_config "$1" "$2" "$3" >"$work/$2.config" || return 1
  read -r count fault bad <"$work/$2.config" || return 1
  [ "$bad" -eq 0 ] && cmp -s "$4" "$work/$2.norm"
}

run_tlc() {
  mkdir "$1/jtmp" || return 1
  (
    cd "$1" || exit 1
    "$java" -Xmx2g -XX:+UseParallelGC -Djava.io.tmpdir=jtmp -cp ../tla2tools.jar tlc2.TLC \
      -workers "$4" -metadir meta -config "$3" "$2" >out.raw 2>&1 </dev/null
  )
  status=$?
  tr -d '\r' <"$1/out.raw" >"$artifacts/${1##*/}.txt" || return 1
  awk -v banner="$tlc_banner" '
    $0 == banner { banners++ }
    /^[0-9]+ states generated, [0-9]+ distinct states found, [0-9]+ states left on queue\.$/ { lines++; distinct = $4; left = $8 }
    /^Error: Invariant [A-Za-z0-9_]+ is violated\.$/ { broken = broken (broken == "" ? "" : ",") $3 }
    $0 == "Model checking completed. No error has been found." { done = 1 }
    END {
      if (lines != 1) { distinct = "-"; left = "-" }
      printf "%s %s %d %s %d\n", distinct, left, done, (broken == "" ? "-" : broken), banners
    }' "$artifacts/${1##*/}.txt" >"$1/summary.txt" || return 1
  read -r distinct left finished broken banners <"$1/summary.txt" || return 1
  [ "$banners" = 1 ]
}

spend() {
  states=-
  listed=-
  violated=-
  if ! present Hold.tla Spend.tla Spend.cfg; then
    refuse model spend-files "Hold.tla, Spend.tla or Spend.cfg is not in the commit"
  elif ! config_kept "$work/in/Spend.cfg" spend "$invariants" "$work/config.want" || [ "$fault" != '"none"' ]; then
    refuse model spend-config "Spend.cfg is not the configuration of T7.6 that the script holds, with S1 to S11 and Fault none"
  else
    mkdir "$work/spend" || setup_failed
    copy_model Spend.cfg "$work/spend"
    if run_tlc "$work/spend" Spend.tla Spend.cfg "$workers"; then
      if [ "$status" -eq 0 ] && [ "$finished" -eq 1 ] && [ "$left" = 0 ] && [ "$broken" = - ]; then
        states=$distinct
        listed=$count
        violated=no
      elif [ "$broken" != - ] && [ "$distinct" != - ]; then
        states=$distinct
        listed=$count
        violated=yes
      fi
    fi
    if tree_kept model spend; then
      case $violated in
        no) ;;
        yes) refuse model spend-violated "TLC reports the invariant $broken violated" ;;
        *) refuse model spend-run "TLC did not finish Spend.cfg with one banner, one states line and its completion message" ;;
      esac
    fi
  fi
  printf 'formal states=%s invariants=%s violated=%s\n' "$states" "$listed" "$violated"
}

module() {
  {
    printf -- '---- MODULE Committed ----\nEXTENDS Hold\nTraces == <<\n'
    sed -e 1d -e 's/\[/<</g' -e 's/\]/>>/g' -e 's/{/[/g' -e 's/}/]/g' \
      -e 's/"\([a-z]*\)":/\1 |-> /g' -e 's/null/Null/g' -e '$!s/$/,/' "$1"
    printf '>>\n====\n'
  } >"$2/Committed.tla" || setup_failed
}

replay_files() {
  present Hold.tla Replay.tla Replay.cfg && config_kept "$work/in/Replay.cfg" replay "" "$work/replay-config.want"
}

told() {
  told=-
  if run_tlc "$1" Replay.tla Replay.cfg 1 && [ "$status" -eq 0 ]; then
    told=$(sed -n 's/^"replay count=\([0-9]*\) accepted=\([0-9]*\) pairs=\([0-9]*\) causes=\([0-9]*\)"$/\1 \2 \3 \4/p' "$artifacts/${1##*/}.txt")
    case $told in
      '' | *"
"*) told=- ;;
    esac
  fi
}

replay_dir() {
  mkdir "$1" || setup_failed
  cp "$work/in/Hold.tla" "$work/in/Replay.cfg" "$1/" || setup_failed
  cp "$2" "$1/Replay.tla" || setup_failed
}

form() {
  awk '
    NR == 1 {
      if ($0 !~ /^\{"seed":[1-9][0-9]*,"count":[1-9][0-9]*\}$/) bad++
      header = $0
      sub(/^.*"count":/, "", header)
      sub(/\}$/, "", header)
      next
    }
    {
      moves = $0
      n = gsub(/\{"move":"(reserve|settle)","arg":[0-3],"answer":"[a-z_]+","state":"[a-z_]+","spent":(0|[1-9][0-9]*),"held":(0|[1-9][0-9]*),"cost":(0|[1-9][0-9]*)\}|\{"move":"(dispatch|void|expire)","arg":null,"answer":"[a-z_]+","state":"[a-z_]+","spent":(0|[1-9][0-9]*),"held":(0|[1-9][0-9]*),"cost":(0|[1-9][0-9]*)\}/, "M", moves)
      count++
      if (n != (count <= 6 ? 5 : 12)) bad++
      if (moves !~ /^\{"cap":(null|[0-3]),"active":"(yes|no)","spent":[0-2],"held":[0-2],"moves":\[M(,M)*\]\}$/) bad++
    }
    END {
      if (NR < 2 || header + 0 != count || count < 206) bad++
      print (bad ? "-" : count)
    }' "$1"
}

replay() {
  traces=-
  accepted=-
  pairs=-
  causes=-
  lines=$(form "$work/in/traces.jsonl") || lines=-
  if ! present traces.jsonl Hold.tla Replay.tla Replay.cfg; then
    refuse traces replay-files "traces.jsonl, Hold.tla, Replay.tla or Replay.cfg is not in the commit"
  elif ! replay_files; then
    refuse traces replay-config "Replay.cfg is not the two lines CONSTANTS and Null = Null"
  elif [ "$lines" = - ]; then
    refuse traces traces-form "traces.jsonl is not of the form of T7.6: header, ranges of Export.cfg, 5 and 12 moves, at least 206 traces"
  else
    replay_dir "$work/replay" "$work/in/Replay.tla"
    module "$work/in/traces.jsonl" "$work/replay"
    told "$work/replay"
    if tree_kept traces replay; then
      if [ "$told" = - ]; then
        refuse traces replay-run "the replay did not finish with one replay line"
      else
        read -r traces accepted pairs causes <<EOF
$told
EOF
        if [ "$traces" != "$lines" ] || [ "$accepted" != "$traces" ] || [ "$pairs" != 30 ] || [ "$causes" != 8 ]; then
          refuse traces replay-values "the replay does not accept every trace of the file with 30 pairs and 8 causes"
        fi
      fi
    fi
  fi
  printf 'formal traces count=%s accepted=%s pairs=%s causes=%s\n' "$traces" "$accepted" "$pairs" "$causes"
}

control_group() {
  group_count=-
  group_accepted=-
  if replay_files; then
    printf '%s\n' "$3" >"$work/$1.jsonl" || setup_failed
    replay_dir "$work/$1" "$2"
    module "$work/$1.jsonl" "$work/$1"
    told "$work/$1"
    if tree_kept traces "$1" && [ "$told" != - ]; then
      read -r group_count group_accepted _ _ <<EOF
$told
EOF
    fi
  fi
}

replay_control() {
  control_group "${3:-replay}-right" "$1" "$right_traces"
  right="$group_count $group_accepted"
  control_group "${3:-replay}-wrong" "$1" "$wrong_traces"
  wrong="$group_count $group_accepted"
  [ -n "${3:-}" ] || printf 'formal replay control right count=%s accepted=%s\nformal replay control wrong count=%s accepted=%s\n' \
    "${right% *}" "${right#* }" "${wrong% *}" "${wrong#* }"
  control_verdict "${2:-replay-control}"
}

control_verdict() {
  if [ "$right" != "$right_count $right_count" ] || [ "$wrong" != "$wrong_count 0" ]; then
    refuse traces "$1" "the replay control: right $right of $right_count accepted, wrong $wrong of $wrong_count accepted where 0 must be"
  fi
}

control() {
  violated=-
  if ! present Hold.tla Spend.tla "$2"; then
    refuse control "control-$1-files" "Hold.tla, Spend.tla or $2 is not in the commit"
  elif ! config_kept "$work/in/$2" "control-$1" "$3" "$work/config.want" || [ "$fault" != "\"$4\"" ]; then
    refuse control "control-$1-config" "$2 is not the configuration of T7.6 with Fault $4 and the one invariant $3"
  else
    mkdir "$work/control-$1" || setup_failed
    copy_model "$2" "$work/control-$1"
    if run_tlc "$work/control-$1" Spend.tla "$2" "$workers"; then
      if [ "$status" -eq 12 ] && [ "$broken" = "$3" ]; then
        violated=yes
      elif [ "$status" -eq 0 ] && [ "$finished" -eq 1 ] && [ "$broken" = - ]; then
        violated=no
      fi
    fi
    if tree_kept control "control-$1"; then
      case $violated in
        yes) rejected=$((rejected + 1)) ;;
        no) refuse control "control-$1-holds" "TLC finds $3 not violated" ;;
        *) refuse control "control-$1-run" "TLC did not finish $2 with one banner and the violation of $3" ;;
      esac
    fi
  fi
  printf 'formal control=%s invariant=%s violated=%s\n' "$1" "$3" "$violated"
}

copy_model() {
  for file in Hold.tla Spend.tla "$1"; do
    cp "$work/in/$file" "$2/" || setup_failed
  done
}

skipped() {
  printf 'formal states=- invariants=- violated=-\n'
  printf 'formal traces count=- accepted=- pairs=- causes=-\n'
  printf 'formal replay control right count=- accepted=-\n'
  printf 'formal replay control wrong count=- accepted=-\n'
  printf 'formal control=ruling invariant=S1 violated=-\n'
  printf 'formal control=resend invariant=S5 violated=-\n'
}

measure() {
  enter_repo
  read_inputs
  count_workers
  rejected=0
  if prepare; then
    printf 'formal workers=%s\n' "$workers"
    spend
    replay
    replay_control "$work/in/Replay.tla"
    control ruling ControlRuling.cfg S1 grant_without_ruling
    control resend ControlResend.cfg S5 resend_new_id
  else
    skipped
  fi
  if [ -n "$cause" ]; then
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

java_version() {
  printf '%s version "%s" %s' "$1" "$2" "$3"
}

note() {
  printf 'fixture %s want=%s got=%s\n' "$1" "$2" "$3"
  [ "$2" != FAIL ] || failing=$((failing + 1))
  if [ "$3" != "$2" ]; then
    wrongs="$wrongs $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

with_own_cause() {
  saved=$cause
  cause=
  "$@"
  got_cause=$cause
  cause=$saved
}

replace_once() {
  OLD=$2 NEW=$3 awk '
    BEGIN { old = ENVIRON["OLD"]; new = ENVIRON["NEW"] }
    { s = s $0 "\n" }
    END {
      t = s
      while ((i = index(t, old)) > 0) { n++; t = substr(t, i + length(old)) }
      if (n != 1) exit 3
      i = index(s, old)
      printf "%s", substr(s, 1, i - 1) new substr(s, i + length(old))
    }' "$1" >"$4"
}

replay_wrong_alone() {
  right=$head_right
  control_group "selftest-$1-wrong" "$work/replay-$1.tla" "$wrong_traces"
  wrong="$group_count $group_accepted"
  control_verdict "replay-control-$1"
}

replay_fixture() {
  if ! replace_once "$work/in/Replay.tla" '[same |-> told.same /\ same,' "$2" "$work/replay-$1.tla"; then
    note "replay-$1" FAIL no-match
    return
  fi
  with_own_cause replay_wrong_alone "$1"
  if [ "$got_cause" = traces ]; then
    note "replay-$1" FAIL FAIL
  else
    note "replay-$1" FAIL "${got_cause:-PASS}"
  fi
}

listener_fixture() {
  d=$work/listen
  if ! mkdir "$d" || ! mkfifo "$d/hold"; then
    note silent-address FAIL setup
    return
  fi
  tries=0
  alive=no
  while [ "$tries" -lt 3 ] && [ "$alive" = no ]; do
    port=$((20000 + ($$ + tries * 7919) % 20000))
    nc -l 127.0.0.1 "$port" <"$d/hold" >"$d/received" 2>&1 &
    listener=$!
    exec 9>"$d/hold"
    sleep 1
    if kill -0 "$listener" 2>/dev/null; then
      alive=yes
    else
      exec 9>&-
      tries=$((tries + 1))
    fi
  done
  if [ "$alive" = no ]; then
    note silent-address FAIL no-listener
    return
  fi
  start=$(date +%s)
  if fetch_jar "https://127.0.0.1:$port/tla2tools.jar" 1 0 0 "$d/tla2tools.jar"; then got=PASS; else got=FAIL; fi
  end=$(date +%s)
  exec 9>&-
  kill "$listener" 2>/dev/null
  wait "$listener" 2>/dev/null
  if [ $((end - start)) -lt 1 ] || [ $((end - start)) -gt 5 ] || [ -e "$d/tla2tools.jar" ]; then
    got="seconds-$((end - start))"
  fi
  note silent-address FAIL "$got"
}

fx_stubs() {
  cat >"$fx/bin/java" <<'EOF'
#!/bin/sh
[ -z "${JAVA_TOOL_OPTIONS:-}${JDK_JAVA_OPTIONS:-}${_JAVA_OPTIONS:-}" ] || exit 96
if [ "$*" = -version ]; then
  printf '%s\n' "$STUB_VERSION" >&2
  exit 0
fi
here=${PWD##*/}
cfg=
module=
for a in "$@"; do
  cfg=$module
  module=$a
done
w=$STUB_WORKERS
[ "$cfg" != Replay.cfg ] || w=1
[ "$*" = "-Xmx2g -XX:+UseParallelGC -Djava.io.tmpdir=jtmp -cp ../tla2tools.jar tlc2.TLC -workers $w -metadir meta -config $cfg $module" ] || exit 97
[ -d jtmp ] && [ -f ../tla2tools.jar ] && [ -f "$cfg" ] && [ -f "$module" ] && [ -f Hold.tla ] || exit 98
printf 'run:%s\n' "$here" >>"$STUB_DIR/log"
[ "$STUB_WRITE_AT" != "$here" ] || printf 'changed\n' >>"$STUB_REPO/formal/r6-spend/README.md"
cat "$STUB_DIR/out.$here"
exit "$(cat "$STUB_DIR/code.$here")"
EOF
  cat >"$fx/bin/curl" <<'EOF'
#!/bin/sh
out=
prev=
args=
for a in "$@"; do
  if [ "$prev" = -o ]; then
    out=$a
    a=OUT
  fi
  args="$args $a"
  prev=$a
done
[ "$args" = " -q --proto =https --proto-redir =https -fsS -L --connect-timeout $STUB_CONNECT --max-time $STUB_MAX --retry $STUB_RETRIES --retry-delay $STUB_DELAY -o OUT --url $STUB_URL" ] || exit 97
case $out in
  /*/tla2tools.jar) ;;
  *) exit 98 ;;
esac
printf 'curl\n' >>"$STUB_DIR/log"
[ "$STUB_WRITE_AT" != curl ] || printf 'changed\n' >>"$STUB_REPO/formal/r6-spend/README.md"
[ "$STUB_CURL_EXIT" = 0 ] || exit "$STUB_CURL_EXIT"
cp "$STUB_JAR" "$out"
EOF
  cat >"$fx/bin/getconf" <<'EOF'
#!/bin/sh
[ "$*" = _NPROCESSORS_ONLN ] || exit 2
[ -n "$STUB_CORES" ] || exit 1
printf '%s\n' "$STUB_CORES"
EOF
  cat >"$fx/jh11/bin/java" <<'EOF'
#!/bin/sh
STUB_VERSION='openjdk version "11.0.2" 2019-01-15'
export STUB_VERSION
exec "$STUB_BIN/java" "$@"
EOF
  cat >"$fx/badbin/java" <<'EOF'
#!/bin/sh
exit 99
EOF
  cat >"$repo_fx/jhrel/bin/java" <<'EOF'
#!/bin/sh
printf 'wrapped\n' >>"$STUB_DIR/log"
exec "$STUB_BIN/java" "$@"
EOF
  cp "$fx/bin/java" "$fx/jh/bin/java" &&
    chmod +x "$fx/bin/java" "$fx/bin/curl" "$fx/bin/getconf" "$fx/jh/bin/java" "$fx/jh11/bin/java" "$fx/badbin/java" "$repo_fx/jhrel/bin/java"
}

fx_init() {
  fx=$work/fixture
  repo_fx=$fx/repo
  mkdir -p "$fx/bin" "$fx/badbin" "$fx/jh/bin" "$fx/jh11/bin" "$fx/runs" "$repo_fx/$model" || return 1
  cp "$work/in/"* "$repo_fx/$model/" || return 1
  git -C "$repo_fx" init -q >"$fx/init.txt" 2>&1 &&
    git -C "$repo_fx" add -A >>"$fx/init.txt" 2>&1 &&
    git -C "$repo_fx" commit -q -m base >>"$fx/init.txt" 2>&1 || return 1
  fx_base=$(git -C "$repo_fx" rev-parse HEAD) || return 1
  mkdir -p "$repo_fx/jhrel/bin" || return 1
  fx_stubs || return 1
  cp "$work/tla2tools.jar" "$fx/jar" && cp "$work/tla2tools.jar" "$fx/jar-longer" &&
    printf 'x' >>"$fx/jar-longer" &&
    dd if=/dev/zero of="$fx/jar-zeros" bs="$jar_bytes" count=1 2>/dev/null || return 1
  STUB_BIN=$fx/bin
  STUB_REPO=$repo_fx
  STUB_URL=$jar_url
  STUB_CONNECT=$fetch_connect
  STUB_MAX=$fetch_max_time
  STUB_RETRIES=$fetch_retries
  STUB_DELAY=$fetch_retry_delay
  export STUB_BIN STUB_REPO STUB_URL STUB_CONNECT STUB_MAX STUB_RETRIES STUB_DELAY
}

fx_out() {
  out_dir=$1
  printf '%s\n' "$2" >"$fsd/code.$out_dir"
  shift 2
  printf '%s\n' "$@" >"$fsd/out.$out_dir"
}

fx_case() {
  fname=$1
  fkind=$2
  fsd=$fx/runs/$fname
  fx_broken=
  if ! mkdir "$fsd" "$fsd/tmp" "$fsd/art" "$fsd/work" "$fsd/work/in" || ! git -C "$repo_fx" reset -q --hard "$fx_base"; then
    note "$fname" PASS fixture-setup
    return 1
  fi
  : >"$fsd/log"
  if [ "$fkind" = step ]; then
    cp "$work/in/"* "$fsd/work/in/" || fx_broken=yes
    fx_target=$fsd/work/in
  else
    fx_target=$repo_fx/$model
  fi
  states_line='8209842 states generated, 1701266 distinct states found, 0 states left on queue.'
  done_line='Model checking completed. No error has been found.'
  fx_out spend 0 "$tlc_banner" "$done_line" "$states_line"
  fx_out replay 0 "$tlc_banner" '"replay count=206 accepted=206 pairs=30 causes=8"' '0 states generated, 0 distinct states found, 0 states left on queue.'
  fx_out replay-right 0 "$tlc_banner" "\"replay count=$right_count accepted=$right_count pairs=12 causes=4\""
  fx_out replay-wrong 0 "$tlc_banner" "\"replay count=$wrong_count accepted=0 pairs=12 causes=4\""
  fx_out control-ruling 12 "$tlc_banner" 'Error: Invariant S1 is violated.' '685 states generated, 212 distinct states found, 144 states left on queue.'
  fx_out control-resend 12 "$tlc_banner" 'Error: Invariant S5 is violated.' '9092 states generated, 2527 distinct states found, 1667 states left on queue.'
  f_repo=$repo_fx
  f_cores=16
  f_workers=4
  f_version=$(java_version openjdk 17.0.2 2022-01-18)
  f_jar=$fx/jar
  f_curl_exit=0
  f_write=
  f_tmpdir=$fsd/tmp
  f_java_home=$fx/jh
  f_path=$fx/bin:$PATH
  f_opts=
  f_missing=
  f_committed=
  w_workers='formal workers=4'
  w_states='formal states=1701266 invariants=11 violated=no'
  w_traces='formal traces count=206 accepted=206 pairs=30 causes=8'
  w_right="formal replay control right count=$right_count accepted=$right_count"
  w_wrong="formal replay control wrong count=$wrong_count accepted=0"
  w_ruling='formal control=ruling invariant=S1 violated=yes'
  w_resend='formal control=resend invariant=S5 violated=yes'
  w_last="PASS $name"
  w_runs='curl run:spend run:replay run:replay-right run:replay-wrong run:control-ruling run:control-resend'
  w_guard=
  w_tree=same
}

fx_dash() {
  w_workers=
  w_states='formal states=- invariants=- violated=-'
  w_traces='formal traces count=- accepted=- pairs=- causes=-'
  w_right='formal replay control right count=- accepted=-'
  w_wrong='formal replay control wrong count=- accepted=-'
  w_ruling='formal control=ruling invariant=S1 violated=-'
  w_resend='formal control=resend invariant=S5 violated=-'
}

fx_alone() {
  w_workers=
  w_states=
  w_traces=
  w_right=
  w_wrong=
  w_ruling=
  w_resend=
  w_runs=
}

fx_fail() {
  w_last="FAIL $name ($1)"
  w_guard=$2
}

fx_edit() {
  if replace_once "$fx_target/$1" "$2" "$3" "$fsd/edit"; then
    cp "$fsd/edit" "$fx_target/$1" || fx_broken=yes
  else
    fx_broken=yes
  fi
}

fx_line() {
  if LINE=$2 OLD=$3 NEW=$4 awk '
    BEGIN { old = ENVIRON["OLD"]; new = ENVIRON["NEW"] }
    NR == ENVIRON["LINE"] + 0 {
      i = index($0, old)
      if (i == 0) bad = 1
      else $0 = substr($0, 1, i - 1) new substr($0, i + length(old))
    }
    { print }
    END { exit bad }' "$fx_target/$1" >"$fsd/edit"; then
    cp "$fsd/edit" "$fx_target/$1" || fx_broken=yes
  else
    fx_broken=yes
  fi
}

fx_awk() {
  if awk "$2" "$fx_target/$1" >"$fsd/edit"; then
    cp "$fsd/edit" "$fx_target/$1" || fx_broken=yes
  else
    fx_broken=yes
  fi
}

fx_move() {
  git -C "$repo_fx" mv "$model/$1" "$model/$1.moved" >/dev/null 2>&1 || fx_broken=yes
}

fx_commit() {
  git -C "$repo_fx" add -A -- "$model" >/dev/null 2>&1 &&
    git -C "$repo_fx" commit -q --allow-empty -m "$fname" >/dev/null 2>&1 || fx_broken=yes
}

fx_stub_env() {
  STUB_DIR=$fsd
  STUB_CORES=$f_cores
  STUB_WORKERS=$f_workers
  STUB_VERSION=$f_version
  STUB_JAR=$f_jar
  STUB_CURL_EXIT=$f_curl_exit
  STUB_WRITE_AT=$f_write
  export STUB_DIR STUB_CORES STUB_WORKERS STUB_VERSION STUB_JAR STUB_CURL_EXIT STUB_WRITE_AT
  git -C "$repo_fx" status --porcelain --untracked-files=all >"$fsd/status.before" 2>&1
}

fx_run() {
  [ "$f_committed" = yes ] || fx_commit
  fx_stub_env
  (
    cd "$fx" || exit 1
    env CI_D_REPO="$f_repo" CI_D_ARTIFACTS="$fsd/art" TMPDIR="$f_tmpdir" PATH="$f_path" R6_FORMAL_NESTED=1 \
      JAVA_HOME="$f_java_home" JAVA_TOOL_OPTIONS="$f_opts" JDK_JAVA_OPTIONS="$f_opts" _JAVA_OPTIONS="$f_opts" \
      sh "$self" "$@"
  ) >"$fsd/stdout" 2>"$fsd/stderr"
  f_code=$?
  for wanted_line in "$w_workers" "$w_states" "$w_traces" "$w_right" "$w_wrong" "$w_ruling" "$w_resend" "$w_last"; do
    [ -z "$wanted_line" ] || printf '%s\n' "$wanted_line"
  done >"$fsd/want"
  case $w_last in
    FAIL*) want=FAIL ;;
    *) want=PASS ;;
  esac
  want_code=0
  [ "$want" = PASS ] || want_code=1
  fx_check
}

fx_step() {
  printf '%s\n' "$config" >"$fsd/work/config.want"
  printf '%s\n' "$replay_config" >"$fsd/work/replay-config.want"
  : >"$fsd/work/tla2tools.jar"
  fx_stub_env
  s_path=$PATH
  PATH=$f_path
  s_java_home=${JAVA_HOME-}
  s_java_home_set=${JAVA_HOME+set}
  if [ -n "$f_java_home" ]; then
    JAVA_HOME=$f_java_home
    export JAVA_HOME
  else
    unset JAVA_HOME
  fi
  s_work=$work
  s_artifacts=$artifacts
  s_repo=$repo
  s_java=$java
  s_workers=$workers
  s_cause=$cause
  s_rejected=$rejected
  s_missing=$missing
  work=$fsd/work
  artifacts=$fsd/art
  repo=$repo_fx
  java=$fx/bin/java
  workers=4
  cause=
  rejected=0
  missing=$f_missing
  if tree_state "$work/tree.before"; then
    "$@" >"$fsd/stdout" 2>"$fsd/stderr"
    printf 'cause=%s rejected=%s\n' "$cause" "$rejected" >>"$fsd/stdout"
  fi
  work=$s_work
  artifacts=$s_artifacts
  repo=$s_repo
  java=$s_java
  workers=$s_workers
  cause=$s_cause
  rejected=$s_rejected
  missing=$s_missing
  PATH=$s_path
  if [ -n "$s_java_home_set" ]; then
    JAVA_HOME=$s_java_home
    export JAVA_HOME
  else
    unset JAVA_HOME
  fi
  want=PASS
  [ -z "$w_guard" ] || want=FAIL
  f_code=0
  want_code=0
  fx_check
}

fx_prepare() {
  w_guard=$2
  fx_want "cause=$1 rejected=0"
  fx_step prepare
}

fx_want() {
  printf '%s\n' "$@" >"$fsd/want"
}

fx_check() {
  got=$want
  log=$(tr '\n' ' ' <"$fsd/log")
  [ "${log% }" = "$w_runs" ] || got=runs
  [ -z "$w_guard" ] || [ -f "$fsd/art/refused-$w_guard.txt" ] || got=guard
  git -C "$repo_fx" status --porcelain --untracked-files=all >"$fsd/status.after" 2>&1
  if [ "$w_tree" = same ]; then
    cmp -s "$fsd/status.before" "$fsd/status.after" || got=tree
  fi
  cmp -s "$fsd/want" "$fsd/stdout" || got=lines
  [ "$f_code" = "$want_code" ] || got="exit-$f_code"
  [ -z "$fx_broken" ] || got='edit-did-not-apply'
  note "$fname" "$want" "$got"
}

flow_fixtures() {
  fx_case good nested && fx_run
  fx_case crlf-output nested && {
    for out_dir in spend replay replay-right replay-wrong control-ruling control-resend; do
      awk '{ printf "%s\r\n", $0 }' "$fsd/out.$out_dir" >"$fsd/crlf" && cp "$fsd/crlf" "$fsd/out.$out_dir"
    done
    fx_run
  }
  fx_case java-options-unset nested && f_opts=-Xmx1g && fx_run
  fx_case worktree-not-read nested && fx_commit && f_committed=yes && printf '{"seed":3,"count":206}\n' >"$repo_fx/$model/traces.jsonl" && fx_run
  fx_case two-causes nested && fx_out replay 0 "$tlc_banner" '"replay count=206 accepted=205 pairs=30 causes=8"' &&
    fx_out control-ruling 0 "$tlc_banner" "$done_line" "$states_line" &&
    w_traces='formal traces count=206 accepted=205 pairs=30 causes=8' && w_ruling='formal control=ruling invariant=S1 violated=no' &&
    fx_fail traces replay-values && fx_run
  fx_case java-home-first nested && f_path=$fx/badbin:$fx/bin:$PATH && fx_run
  fx_case java-version-second-line nested && f_java_home= && f_version="NOTE: picked up nothing
$(java_version openjdk 21.0.4 2024-07-16)" && fx_run
  fx_case java-home-relative nested && f_java_home=jhrel &&
    w_runs='curl wrapped wrapped run:spend wrapped run:replay wrapped run:replay-right wrapped run:replay-wrong wrapped run:control-ruling wrapped run:control-resend' && fx_run
  fx_case pin-other-hash-whole nested && fx_edit README.md "sha256=$jar_sha256" "sha256=${jar_sha256%?}9" && fx_dash && w_runs= && fx_fail hash pin && fx_run
  fx_case jar-same-size-whole nested && f_jar=$fx/jar-zeros && fx_dash && w_runs=curl && fx_fail hash hash && fx_run
  fx_case fetch-fails-whole nested && f_curl_exit=22 && fx_dash && w_runs=curl && fx_fail fetch fetch && fx_run
  fx_case java-path-11-whole nested && f_java_home= && f_version=$(java_version openjdk 11.0.2 2019-01-15) && fx_dash && w_runs=curl && fx_fail java java && fx_run
  fx_case configs-override-in-all-three nested && {
    for file in Spend.cfg ControlRuling.cfg ControlResend.cfg; do
      fx_edit "$file" '    Fault =' '    S2 <- S1
    Fault ='
    done
    w_states='formal states=- invariants=- violated=-'
    w_ruling='formal control=ruling invariant=S1 violated=-'
    w_resend='formal control=resend invariant=S5 violated=-'
    w_runs='curl run:replay run:replay-right run:replay-wrong'
    fx_fail model spend-config
    fx_run
  }

  fx_case pin-other-hash step && fx_edit README.md "sha256=$jar_sha256" "sha256=${jar_sha256%?}9" && w_runs= && fx_prepare hash pin
  fx_case pin-other-host step && fx_edit README.md "tlc url=$jar_url" 'tlc url=https://example.invalid/tla2tools.jar' && w_runs= && fx_prepare hash pin
  fx_case pin-option-file step && fx_edit README.md "tlc url=$jar_url" 'tlc url=-Kcurl.cfg' &&
    printf 'output = "formal/r6-spend/README.md"\nurl = "file:///dev/null"\n' >"$fsd/work/curl.cfg" && w_runs= && fx_prepare hash pin
  fx_case pin-version-option step && fx_edit README.md "tlc url=$jar_url" 'tlc url=--version' && w_runs= && fx_prepare hash pin
  fx_case pin-other-bytes step && fx_edit README.md "bytes=$jar_bytes" 'bytes=2274533' && w_runs= && fx_prepare hash pin
  fx_case pin-line-indented step && fx_edit README.md '
tlc url=' '
  tlc url=' && w_runs= && fx_prepare hash pin
  fx_case pin-line-prefixed step && fx_edit README.md '
tlc url=https://' '
htlc url=ttps://' && w_runs= && fx_prepare hash pin
  fx_case pin-line-absent step && fx_edit README.md '
tlc url=' '
tlc-url=' && w_runs= && fx_prepare hash pin
  fx_case pin-line-twice step && fx_edit README.md '
tlc url=' "
tlc url=$jar_url bytes=$jar_bytes sha256=$jar_sha256
tlc url=" && w_runs= && fx_prepare hash pin
  fx_case jar-same-size step && f_jar=$fx/jar-zeros && w_runs=curl && fx_prepare hash hash
  fx_case jar-longer step && f_jar=$fx/jar-longer && w_runs=curl && fx_prepare hash hash
  fx_case fetch-fails step && f_curl_exit=22 && w_runs=curl && fx_prepare fetch fetch
  fx_case tree-after-fetch step && f_write=curl && w_tree=changed && w_runs=curl && fx_prepare fetch tree-fetch
  fx_case java-home-old step && f_java_home=$fx/jh11 && w_runs=curl && fx_prepare java java
  fx_case java-home-empty step && f_java_home=$fx/none && w_runs=curl && fx_prepare java java
  fx_case java-path-11 step && f_java_home= && f_version=$(java_version openjdk 11.0.2 2019-01-15) && w_runs=curl && fx_prepare java java
  fx_case java-path-8 step && f_java_home= && f_version=$(java_version java 1.8.0_292 '') && w_runs=curl && fx_prepare java java
  fx_case tmpdir-relative nested && f_tmpdir=formal && fx_alone && fx_fail setup '' && fx_run
  fx_case tmpdir-relative-outside nested && f_tmpdir="../runs/$fname/tmp" && fx_alone && fx_fail setup '' && fx_run
  fx_case tmpdir-inside-checkout nested && f_tmpdir=$repo_fx/formal && fx_alone && fx_fail setup '' && fx_run
  fx_case repository-absent nested && f_repo=$fx/absent && fx_alone && fx_fail setup '' && fx_run
  fx_case usage-second-argument nested && fx_alone && fx_fail usage '' && fx_run --self-test x
  fx_case usage-other-argument nested && fx_alone && fx_fail usage '' && fx_run --all
}

workers_line() {
  count_workers
  printf 'formal workers=%s\n' "$workers"
}

worker_fixtures() {
  for cores in 16 4 3 1 '' 0 x; do
    fx_case "workers-cores-${cores:-none}" step || continue
    f_cores=$cores
    case $cores in
      16 | 4) workers_want=4 ;;
      3) workers_want=3 ;;
      *) workers_want=1 ;;
    esac
    w_runs=
    fx_want "formal workers=$workers_want" 'cause= rejected=0'
    fx_step workers_line
  done
}

spend_fixtures() {
  states_dash='formal states=- invariants=- violated=-'
  fx_case spend-good step && w_runs=run:spend && fx_want 'formal states=1701266 invariants=11 violated=no' 'cause= rejected=0' && fx_step spend
  for edit in comment block-comment twice other-name fault override assignment bound constraint next; do
    fx_case "spend-config-$edit" step || continue
    case $edit in
      comment) fx_edit Spend.cfg '
    S4
' '
    \* S4
' ;;
      block-comment) fx_edit Spend.cfg 'INIT Init' '(* a note *)
INIT Init' ;;
      twice) fx_edit Spend.cfg '
    S11
' '
    S1
' ;;
      other-name) fx_edit Spend.cfg '
    S11
' '
    S12
' ;;
      fault) fx_edit Spend.cfg 'Fault = "none"' 'Fault = "third_send_same_id"' ;;
      override) fx_edit Spend.cfg '    Fault =' '    S2 <- S1
    Fault =' ;;
      assignment) fx_edit Spend.cfg '    Fault =' '    S4 = TRUE
    Fault =' ;;
      bound) fx_edit Spend.cfg 'MaxRequests = 2' 'MaxRequests = 1' ;;
      constraint) fx_edit Spend.cfg 'INIT Init' 'CONSTRAINT Small
INIT Init' ;;
      next) fx_edit Spend.cfg 'NEXT Next' 'NEXT XNext' ;;
    esac
    w_runs=
    w_guard=spend-config
    fx_want "$states_dash" 'cause=model rejected=0'
    fx_step spend
  done
  fx_case spend-config-trailing-comment step && fx_edit Spend.cfg '    Hurry = TRUE
' '    Hurry = TRUE \* the bound of SP-1
' && w_runs=run:spend && fx_want 'formal states=1701266 invariants=11 violated=no' 'cause= rejected=0' && fx_step spend
  fx_case spend-file-absent step && f_missing=' Spend.cfg' && w_runs= && w_guard=spend-files && fx_want "$states_dash" 'cause=model rejected=0' && fx_step spend
  for edit in status-1 no-completion quoted-completion left-on-queue two-states-lines no-banner two-banners; do
    fx_case "spend-$edit" step || continue
    case $edit in
      status-1) fx_out spend 1 "$tlc_banner" "$done_line" "$states_line" ;;
      no-completion) fx_out spend 0 "$tlc_banner" "$states_line" ;;
      quoted-completion) fx_out spend 0 "$tlc_banner" "\"$done_line\"" "$states_line" ;;
      left-on-queue) fx_out spend 0 "$tlc_banner" "$done_line" '8209842 states generated, 1701266 distinct states found, 5 states left on queue.' ;;
      two-states-lines) fx_out spend 0 "$tlc_banner" "$done_line" "$states_line" "$states_line" ;;
      no-banner) fx_out spend 0 "$done_line" "$states_line" ;;
      two-banners) fx_out spend 0 "$tlc_banner" "$tlc_banner" "$done_line" "$states_line" ;;
    esac
    w_runs=run:spend
    w_guard=spend-run
    fx_want "$states_dash" 'cause=model rejected=0'
    fx_step spend
  done
  fx_case spend-violated step && fx_out spend 12 "$tlc_banner" 'Error: Invariant S4 is violated.' '300 states generated, 99 distinct states found, 7 states left on queue.' &&
    w_runs=run:spend && w_guard=spend-violated && fx_want 'formal states=99 invariants=11 violated=yes' 'cause=model rejected=0' && fx_step spend
  fx_case spend-violation-beside-completion step && fx_out spend 0 "$tlc_banner" "$done_line" 'Error: Invariant S4 is violated.' "$states_line" &&
    w_runs=run:spend && w_guard=spend-violated && fx_want 'formal states=1701266 invariants=11 violated=yes' 'cause=model rejected=0' && fx_step spend
  fx_case spend-tree step && f_write=spend && w_tree=changed && w_runs=run:spend && w_guard=tree-spend &&
    fx_want 'formal states=1701266 invariants=11 violated=no' 'cause=model rejected=0' && fx_step spend
}

replay_fixtures() {
  traces_dash='formal traces count=- accepted=- pairs=- causes=-'
  fx_case replay-good step && w_runs=run:replay && fx_want 'formal traces count=206 accepted=206 pairs=30 causes=8' 'cause= rejected=0' && fx_step replay
  for edit in header-form header-count cut-short one-dropped drawn-one-move directed-one-move cap leading-zero leading-zero-held start-spent reserve-arg dispatch-arg move-name carriage-return; do
    fx_case "traces-$edit" step || continue
    case $edit in
      header-form) fx_line traces.jsonl 1 '{"seed":3,' '{"seed":"3",' ;;
      header-count) fx_line traces.jsonl 1 '"count":206}' '"count":207}' ;;
      cut-short) fx_awk traces.jsonl 'NR < 100' && fx_line traces.jsonl 1 '"count":206}' '"count":98}' ;;
      one-dropped) fx_awk traces.jsonl '{ line[NR] = $0 } END { for (i = 1; i < NR; i++) print line[i] }' && fx_line traces.jsonl 1 '"count":206}' '"count":205}' ;;
      drawn-one-move) fx_awk traces.jsonl 'NR == 8 { i = index($0, "},{\"move\""); $0 = substr($0, 1, i) "]}" } { print }' ;;
      directed-one-move) fx_awk traces.jsonl 'NR == 2 { i = index($0, "},{\"move\""); $0 = substr($0, 1, i) "]}" } { print }' ;;
      cap) fx_line traces.jsonl 2 '{"cap":3,' '{"cap":7,' ;;
      leading-zero) fx_line traces.jsonl 2 '"arg":2,' '"arg":02,' ;;
      leading-zero-held) fx_line traces.jsonl 2 '"held":3,' '"held":03,' ;;
      start-spent) fx_line traces.jsonl 2 '"active":"yes","spent":0,' '"active":"yes","spent":3,' ;;
      reserve-arg) fx_line traces.jsonl 2 '"move":"reserve","arg":2,' '"move":"reserve","arg":4,' ;;
      dispatch-arg) fx_line traces.jsonl 2 '"move":"dispatch","arg":null,' '"move":"dispatch","arg":1,' ;;
      move-name) fx_line traces.jsonl 2 '"move":"dispatch",' '"move":"send",' ;;
      carriage-return) fx_awk traces.jsonl 'NR == 2 { $0 = $0 "\r" } { print }' ;;
    esac
    w_runs=
    w_guard='traces-form'
    fx_want "$traces_dash" 'cause=traces rejected=0'
    fx_step replay
  done
  fx_case traces-file-absent step && f_missing=' traces.jsonl' && w_runs= && w_guard=replay-files && fx_want "$traces_dash" 'cause=traces rejected=0' && fx_step replay
  fx_case replay-config-null step && fx_edit Replay.cfg 'Null = Null' 'Null = 0' && w_runs= && w_guard=replay-config && fx_want "$traces_dash" 'cause=traces rejected=0' && fx_step replay
  fx_case replay-config-section step && printf 'INVARIANTS\n    S1\n' >>"$fx_target/Replay.cfg" && w_runs= && w_guard=replay-config &&
    fx_want "$traces_dash" 'cause=traces rejected=0' && fx_step replay
  for edit in accepted pairs causes count; do
    fx_case "replay-$edit" step || continue
    case $edit in
      accepted) told_line='count=206 accepted=205 pairs=30 causes=8' ;;
      pairs) told_line='count=206 accepted=206 pairs=29 causes=8' ;;
      causes) told_line='count=206 accepted=206 pairs=30 causes=7' ;;
      count) told_line='count=205 accepted=205 pairs=30 causes=8' ;;
    esac
    fx_out replay 0 "$tlc_banner" "\"replay $told_line\""
    w_runs=run:replay
    w_guard=replay-values
    fx_want "formal traces $told_line" 'cause=traces rejected=0'
    fx_step replay
  done
  fx_case replay-status-10 step && fx_out replay 10 "$tlc_banner" '"replay count=206 accepted=206 pairs=30 causes=8"' &&
    w_runs=run:replay && w_guard=replay-run && fx_want "$traces_dash" 'cause=traces rejected=0' && fx_step replay
  fx_case replay-two-lines step && fx_out replay 0 "$tlc_banner" '"replay count=206 accepted=206 pairs=30 causes=8"' '"replay count=206 accepted=206 pairs=30 causes=8"' &&
    w_runs=run:replay && w_guard=replay-run && fx_want "$traces_dash" 'cause=traces rejected=0' && fx_step replay
  fx_case replay-tree step && f_write=replay && w_tree=changed && w_runs=run:replay && w_guard=tree-replay &&
    fx_want "$traces_dash" 'cause=traces rejected=0' && fx_step replay
}

group_fixtures() {
  right_line="formal replay control right count=$right_count accepted=$right_count"
  wrong_line="formal replay control wrong count=$wrong_count accepted=0"
  groups='run:replay-right run:replay-wrong'
  fx_case control-groups-good step && w_runs=$groups && fx_want "$right_line" "$wrong_line" 'cause= rejected=0' && fx_step replay_control "$fsd/work/in/Replay.tla"
  fx_case control-right-refused step && fx_out replay-right 0 "$tlc_banner" "\"replay count=$right_count accepted=1 pairs=12 causes=4\"" &&
    w_runs=$groups && w_guard=replay-control &&
    fx_want "formal replay control right count=$right_count accepted=1" "$wrong_line" 'cause=traces rejected=0' && fx_step replay_control "$fsd/work/in/Replay.tla"
  fx_case control-wrong-accepted step && fx_out replay-wrong 0 "$tlc_banner" "\"replay count=$wrong_count accepted=1 pairs=12 causes=4\"" &&
    w_runs=$groups && w_guard=replay-control &&
    fx_want "$right_line" "formal replay control wrong count=$wrong_count accepted=1" 'cause=traces rejected=0' && fx_step replay_control "$fsd/work/in/Replay.tla"
  fx_case control-wrong-count step && fx_out replay-wrong 0 "$tlc_banner" '"replay count=6 accepted=0 pairs=12 causes=4"' &&
    w_runs=$groups && w_guard=replay-control &&
    fx_want "$right_line" 'formal replay control wrong count=6 accepted=0' 'cause=traces rejected=0' && fx_step replay_control "$fsd/work/in/Replay.tla"
  fx_case control-groups-config step && fx_edit Replay.cfg 'Null = Null' 'Null = 0' && w_runs= && w_guard=replay-control &&
    fx_want 'formal replay control right count=- accepted=-' 'formal replay control wrong count=- accepted=-' 'cause=traces rejected=0' &&
    fx_step replay_control "$fsd/work/in/Replay.tla"
  fx_case control-groups-tree step && f_write=replay-wrong && w_tree=changed && w_runs=$groups && w_guard=tree-replay-wrong &&
    fx_want "$right_line" 'formal replay control wrong count=- accepted=-' 'cause=traces rejected=0' && fx_step replay_control "$fsd/work/in/Replay.tla"
}

control_fixtures() {
  ruling_dash='formal control=ruling invariant=S1 violated=-'
  fx_case control-ruling-good step && w_runs=run:control-ruling &&
    fx_want 'formal control=ruling invariant=S1 violated=yes' 'cause= rejected=1' && fx_step control ruling ControlRuling.cfg S1 grant_without_ruling
  fx_case control-resend-good step && w_runs=run:control-resend &&
    fx_want 'formal control=resend invariant=S5 violated=yes' 'cause= rejected=1' && fx_step control resend ControlResend.cfg S5 resend_new_id
  for edit in bound other-fault fault-none two-invariants no-invariant; do
    fx_case "control-config-$edit" step || continue
    case $edit in
      bound) fx_edit ControlRuling.cfg 'MaxRequests = 2' 'MaxRequests = 1' ;;
      other-fault) fx_edit ControlRuling.cfg '"grant_without_ruling"' '"grant_kept"' ;;
      fault-none) fx_edit ControlRuling.cfg '"grant_without_ruling"' '"none"' ;;
      two-invariants) fx_edit ControlRuling.cfg '
    S1
' '
    S1
    S2
' ;;
      no-invariant) fx_edit ControlRuling.cfg 'INVARIANTS
    S1
' '' ;;
    esac
    w_runs=
    w_guard=control-ruling-config
    fx_want "$ruling_dash" 'cause=control rejected=0'
    fx_step control ruling ControlRuling.cfg S1 grant_without_ruling
  done
  fx_case control-file-absent step && f_missing=' ControlRuling.cfg' && w_runs= && w_guard=control-ruling-files &&
    fx_want "$ruling_dash" 'cause=control rejected=0' && fx_step control ruling ControlRuling.cfg S1 grant_without_ruling
  for edit in other-invariant status-0 quoted-violation two-violations; do
    fx_case "control-run-$edit" step || continue
    case $edit in
      other-invariant) fx_out control-ruling 12 "$tlc_banner" 'Error: Invariant S2 is violated.' "$states_line" ;;
      status-0) fx_out control-ruling 0 "$tlc_banner" 'Error: Invariant S1 is violated.' "$states_line" ;;
      quoted-violation) fx_out control-ruling 12 "$tlc_banner" '"Error: Invariant S1 is violated."' "$states_line" ;;
      two-violations) fx_out control-ruling 12 "$tlc_banner" 'Error: Invariant S1 is violated.' 'Error: Invariant S1 is violated.' "$states_line" ;;
    esac
    w_runs=run:control-ruling
    w_guard=control-ruling-run
    fx_want "$ruling_dash" 'cause=control rejected=0'
    fx_step control ruling ControlRuling.cfg S1 grant_without_ruling
  done
  fx_case control-holds step && fx_out control-ruling 0 "$tlc_banner" "$done_line" "$states_line" && w_runs=run:control-ruling &&
    w_guard=control-ruling-holds && fx_want 'formal control=ruling invariant=S1 violated=no' 'cause=control rejected=0' &&
    fx_step control ruling ControlRuling.cfg S1 grant_without_ruling
  fx_case control-tree step && f_write=control-ruling && w_tree=changed && w_runs=run:control-ruling && w_guard=tree-control-ruling &&
    fx_want 'formal control=ruling invariant=S1 violated=yes' 'cause=control rejected=0' && fx_step control ruling ControlRuling.cfg S1 grant_without_ruling
}

self_test() {
  if [ -n "${R6_FORMAL_NESTED:-}" ]; then
    printf 'FAIL self-test:%s (nested)\n' "$name"
    exit 1
  fi
  enter_repo
  read_inputs
  count_workers
  rejected=0
  failing=0
  wrongs=
  bound=$(((fetch_retries + 1) * fetch_max_time + fetch_retries * fetch_retry_delay))
  if [ "$bound" -le 60 ]; then note fetch-within-60s PASS PASS; else note fetch-within-60s PASS "seconds-$bound"; fi
  if ! prepare; then
    printf 'formal control=ruling invariant=S1 violated=-\nformal control=resend invariant=S5 violated=-\n'
    printf 'FAIL self-test:%s (%s)\n' "$name" "$cause"
    exit 1
  fi
  failing=$((failing + 2))
  control ruling ControlRuling.cfg S1 grant_without_ruling
  control resend ControlResend.cfg S5 resend_new_id
  with_own_cause replay_control "$work/in/Replay.tla" replay-control-head selftest-head
  head_right=$right
  note replay-head PASS "${got_cause:-PASS}"
  replay_fixture last-move '[same |-> same,'
  replay_fixture first-move '[same |-> told.same /\ (same \/ i > 1),'
  listener_fixture
  if fx_init; then
    flow_fixtures
    worker_fixtures
    spend_fixtures
    replay_fixtures
    group_fixtures
    control_fixtures
  else
    note fixture-repository PASS setup
  fi
  if tree_state "$work/tree.end" && cmp -s "$work/tree.before" "$work/tree.end"; then
    note checkout-unchanged PASS PASS
  else
    note checkout-unchanged PASS changed
  fi
  if [ -n "$cause" ]; then
    printf 'FAIL self-test:%s (%s)\n' "$name" "$cause"
    exit 1
  fi
  if [ -n "$wrongs" ] || [ "$rejected" != "$failing" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s; %s of %s failing fixtures rejected)\n' "$name" "$wrongs" "$rejected" "$failing"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $# in
  0) measure ;;
  *)
    if [ "$#" -eq 1 ] && [ "$1" = --self-test ]; then
      self_test
    else
      printf 'FAIL %s (usage)\n' "$name"
      exit 1
    fi
    ;;
esac
