use strict;
use warnings;

my $seconds = shift @ARGV;
my $pid = fork;
die "limit.pl: fork failed\n" unless defined $pid;
if ($pid == 0) {
    setpgrp(0, 0);
    exec @ARGV;
    exit 127;
}
$SIG{ALRM} = sub {
    kill 'TERM', -$pid;
    sleep 2;
    kill 'KILL', -$pid;
    exit 124;
};
alarm $seconds;
waitpid $pid, 0;
exit(($? >> 8) || (($? & 127) ? 128 + ($? & 127) : 0));
