use strict;
use warnings;

my ($here) = __FILE__ =~ m{^(.*)/};
require((defined $here ? $here : '.') . '/row-places.pl');

sub refuse {
    print "$_[0]\n";
    exit 1;
}

sub reason {
    my ($why) = @_;
    chomp $why;
    return $why;
}

my ($path, $shown) = @ARGV;
my $program = $ENV{ROW_SUBST};
defined $program && defined $path or refuse('the runner gave no substitution or no file');
$shown = $path unless defined $shown;
my @statements = eval { statements_of($program) };
refuse(reason($@)) if $@;

open my $in, '<', $path or do { print "the file could not be read\n"; exit 2 };
my $content = do { local $/; <$in> };
close $in;
$content = '' unless defined $content;
my ($kind, $result) = eval { checked(\@statements, $content, $shown) };
refuse(reason($@)) if $@;
refuse($result) if $kind ne '';
open my $out, '>', $path or do { print "the file could not be written\n"; exit 2 };
print {$out} $result;
close $out or do { print "the file could not be written\n"; exit 2 };
exit 0;
