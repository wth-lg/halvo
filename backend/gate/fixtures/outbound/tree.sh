#!/bin/sh
echo $$ > "$1/root.pid"
sh -c 'echo $$ > "$1/child.pid"; exec sleep 300' sh "$1" &
wait
