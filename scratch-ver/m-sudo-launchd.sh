#!/bin/sh
py=$1
out=$PWD/target/ver/m7-job.txt
launchctl submit -l r2-ver-m6 -- /usr/bin/true
printf 'user submit exit=%s\n' "$?"
sudo -n launchctl submit -l r2-ver-m7 -- "$py" "$PWD/scratch-ver/connect.py" 192.0.2.1 443 "$out"
printf 'root submit exit=%s\n' "$?"
sleep 9
exit 0
