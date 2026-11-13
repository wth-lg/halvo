use strict;
use warnings;

my %closing = ('{' => '}', '(' => ')', '[' => ']', '<' => '>');

sub refuse {
    print "$_[0]\n";
    exit 1;
}

sub delimited {
    my ($text, $at, $open) = @_;
    my $close = exists $closing{$open} ? $closing{$open} : $open;
    my $depth = 1;
    while ($at < length $text) {
        my $c = substr($text, $at, 1);
        if ($c eq "\\") {
            $at += 2;
            next;
        }
        if ($c eq $close) {
            $depth--;
            return $at + 1 if $depth == 0;
        } elsif ($c eq $open) {
            $depth++;
        }
        $at++;
    }
    refuse('the substitution is no sequence of perl s/// statements separated by ;');
}

sub skip_space {
    my ($text, $at) = @_;
    $at++ while $at < length $text && substr($text, $at, 1) =~ /\s/;
    return $at;
}

my $path = shift @ARGV;
my $program = $ENV{ROW_SUBST};
defined $program && defined $path or refuse('the runner gave no substitution or no file');
my @statements;
my $at = skip_space($program, 0);
while ($at < length $program) {
    my $start = $at;
    substr($program, $at, 2) =~ /^s[^\w\s]$/ or refuse('the substitution is no sequence of perl s/// statements separated by ;');
    my $open = substr($program, $at + 1, 1);
    $at = delimited($program, $at + 2, $open);
    if (exists $closing{$open}) {
        $at = skip_space($program, $at);
        my $second = substr($program, $at, 1);
        $second =~ /^[^\w\s]$/ or refuse('the substitution is no sequence of perl s/// statements separated by ;');
        $at = delimited($program, $at + 1, $second);
    } else {
        $at = delimited($program, $at, $open);
    }
    my $flags = $at;
    $at++ while $at < length $program && substr($program, $at, 1) =~ /[a-z]/;
    if (substr($program, $flags, $at - $flags) =~ /r/) {
        refuse(sprintf 'statement %d of the substitution has the r modifier, which changes no file', scalar(@statements) + 1);
    }
    if (substr($program, $flags, $at - $flags) =~ /e/) {
        refuse(sprintf 'statement %d of the substitution has the e modifier: a substitution inside its replacement counts as applied when it matched nothing', scalar(@statements) + 1);
    }
    push @statements, substr($program, $start, $at - $start);
    $at = skip_space($program, $at);
    last if $at >= length $program;
    substr($program, $at, 1) eq ';' or refuse('the substitution is no sequence of perl s/// statements separated by ;');
    $at = skip_space($program, $at + 1);
}
@statements or refuse('the substitution is empty');

open my $in, '<', $path or do { print "the file could not be read\n"; exit 2 };
my $content = do { local $/; <$in> };
close $in;
$_ = $content;
my $index = 0;
for my $statement (@statements) {
    $index++;
    my $count = eval $statement;
    refuse('the substitution is no valid perl') if $@;
    next if $count;
    refuse('the substitution did not apply') if @statements == 1;
    refuse(sprintf 'statement %d of %d of the substitution did not apply', $index, scalar @statements);
}
open my $out, '>', $path or do { print "the file could not be written\n"; exit 2 };
print {$out} $_;
close $out or do { print "the file could not be written\n"; exit 2 };
exit 0;
