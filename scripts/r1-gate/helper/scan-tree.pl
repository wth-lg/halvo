use strict;
use warnings;
use Errno qw(ENOENT ENOTDIR);

my $marker = shift @ARGV;
defined $marker && length $marker or die "usage: scan-tree.pl MARKER ROOT...\n";
my @fields = ('"kind":"', '"seq":', '"app":');
my @wanted = ($marker, @fields);
(my $le = $marker) =~ s/(.)/$1\0/gs;
(my $be = $marker) =~ s/(.)/\0$1/gs;
push @wanted, $le, $be;
my $longest = 0;
for (@wanted) { $longest = length if length > $longest }

sub say_line { print join("\t", @_), "\n" }

sub holds {
    my ($text) = @_;
    for my $w (@wanted) { return 1 if index($text, $w) >= 0 }
    return 0;
}

sub scan_file {
    my ($path, $label, $rel) = @_;
    my $fh;
    unless (open $fh, '<:raw', $path) {
        return if $! == ENOENT;
        say_line('unreadable', $label, $rel);
        return;
    }
    my $tail = '';
    while (1) {
        my $chunk;
        my $got = read $fh, $chunk, 1 << 20;
        unless (defined $got) { say_line('unreadable', $label, $rel); close $fh; return }
        last if $got == 0;
        my $text = $tail . $chunk;
        if (holds($text)) { say_line('hit', $label, $rel); close $fh; return }
        $tail = length $text >= $longest ? substr($text, -($longest - 1)) : $text;
    }
    close $fh;
}

sub walk {
    my ($path, $label, $rel) = @_;
    my @st = lstat $path;
    unless (@st) {
        return if $! == ENOENT || $! == ENOTDIR;
        say_line('unreadable', $label, $rel);
        return;
    }
    say_line('name', $label, $rel) if $rel ne '.' && holds($rel);
    if (-l _) { say_line('link', $label, $rel); return }
    if (-f _) { scan_file($path, $label, $rel); return }
    return unless -d _;
    my $dh;
    unless (opendir $dh, $path) {
        return if $! == ENOENT;
        say_line('unreadable', $label, $rel);
        return;
    }
    my @names = sort grep { $_ ne '.' && $_ ne '..' } readdir $dh;
    closedir $dh;
    for my $name (@names) {
        walk("$path/$name", $label, $rel eq '.' ? $name : "$rel/$name");
    }
}

for my $root (@ARGV) {
    (my $label = $root) =~ s{.*/}{};
    unless (lstat $root) { say_line('missing', $label, '.'); next }
    if (-d _ && !-l _) { walk($root, $label, '.') }
    else { walk($root, $label, $label) }
}
exit 0;
