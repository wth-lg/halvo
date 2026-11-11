#!/bin/sh
py=$1
out=$PWD/target/ver/m1-job.txt
launchctl submit -l r2-ver-m1 -o "$PWD/target/ver/m1-job.out" -e "$PWD/target/ver/m1-job.err" -- "$py" "$PWD/scratch-ver/connect.py" 192.0.2.1 443 "$out"
printf 'submit exit=%s\n' "$?"
sleep 9
exit 0
