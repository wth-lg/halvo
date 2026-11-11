#!/bin/sh
py=$1
out=$PWD/target/ver/m5-cron.txt
printf '* * * * * %s %s/scratch-ver/connect.py 192.0.2.1 443 %s\n' "$py" "$PWD" "$out" | crontab -
printf 'crontab install exit=%s\n' "$?"
crontab -l
exit 0
