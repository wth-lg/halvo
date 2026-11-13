#!/bin/sh
echo "PASS a line from a tool that the check ran"
echo "FAIL another tool line"
echo "a tool printed: PASS good: in the middle of its line"
printf 'a tool printed a zero byte: \000\n'
echo "PASS good: 1 of 1"
