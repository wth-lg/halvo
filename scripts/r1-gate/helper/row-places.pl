use strict;
use warnings;

sub evaluated {
    local $SIG{__WARN__} = sub { };
    local $_ = pop;
    my @result = (scalar(eval pop), $_, $@);
    return @result;
}

use B ();

my %closing = ('{' => '}', '(' => ')', '[' => ']', '<' => '>');
my $not_statements = "the substitution is no sequence of perl s/// statements separated by ;\n";

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
    die $not_statements;
}

sub skip_space {
    my ($text, $at) = @_;
    $at++ while $at < length $text && substr($text, $at, 1) =~ /\s/;
    return $at;
}

sub interpolates {
    my ($code) = @_;
    my ($sub, undef, $error) = evaluated("sub { $code }", '');
    die "the substitution is no valid perl\n" if $error || ref $sub ne 'CODE';
    my @todo = (B::svref_2object($sub)->ROOT);
    while (@todo) {
        my $op = pop @todo;
        next unless $op && ref $op && $$op;
        return 1 if $op->name eq 'regcomp';
        push @todo, $op->sibling;
        push @todo, $op->first if $op->can('first');
    }
    return 0;
}

sub case_escape {
    my ($pattern) = @_;
    while ($pattern =~ /\\(.)/gs) {
        my $escape = $1;
        return $escape if $escape =~ /^[QULulFE]$/;
    }
    return undef;
}

sub variable_in {
    my ($text, $replacement) = @_;
    my $at = 0;
    while ($at < length $text) {
        my $rest = substr($text, $at);
        if ($rest =~ /^\\./s) {
            $at += 2;
        } elsif ($replacement && $rest =~ /^(\$(?:[1-9](?![0-9])|\{[1-9]\}|&))/) {
            $at += length $1;
        } elsif ($replacement && $rest =~ /^(\$(?:\{[^}]*\}?|\w+(?:::\w+)*|.?))/s) {
            return $1;
        } elsif ($rest =~ /^(\@(?:\w+(?:::\w+)*|\{[^}]*\}?|[\$:][\w:]*))/) {
            return $1;
        } else {
            $at++;
        }
    }
    return undef;
}

sub statements_of {
    my ($program) = @_;
    my @statements;
    my $at = skip_space($program, 0);
    while ($at < length $program) {
        my $start = $at;
        substr($program, $at, 2) =~ /^s[^\w\s]$/ or die $not_statements;
        my $open = substr($program, $at + 1, 1);
        my $close = exists $closing{$open} ? $closing{$open} : $open;
        $at = delimited($program, $at + 2, $open);
        my $pattern = substr($program, $start + 2, $at - $start - 3);
        my ($second, $replacement);
        if (exists $closing{$open}) {
            $at = skip_space($program, $at);
            $second = substr($program, $at, 1);
            $second =~ /^[^\w\s]$/ or die $not_statements;
            my $from = $at + 1;
            $at = delimited($program, $from, $second);
            $replacement = substr($program, $from, $at - $from - 1);
        } else {
            $second = $open;
            my $from = $at;
            $at = delimited($program, $from, $open);
            $replacement = substr($program, $from, $at - $from - 1);
        }
        my $flags_at = $at;
        $at++ while $at < length $program && substr($program, $at, 1) =~ /[a-z]/;
        my $flags = substr($program, $flags_at, $at - $flags_at);
        my $index = @statements + 1;
        my $text = substr($program, $start, $at - $start);
        die sprintf("statement %d of the substitution has the r modifier, which changes no file\n", $index) if $flags =~ /r/;
        die sprintf("statement %d of the substitution has the e modifier: a substitution inside its replacement counts as applied when it matched nothing\n", $index) if $flags =~ /e/;
        die sprintf("statement %d of the substitution has an empty pattern, which perl reads as the last pattern that matched\n", $index) if $pattern eq '';
        my $escape = case_escape($pattern);
        die sprintf("statement %d of the substitution has the escape \\%s in its pattern, which perl applies to the pattern before it matches, so the pattern is not the text written: write the text that the row means\n", $index, $escape) if defined $escape;
        my $array = $open eq "'" ? undef : variable_in($pattern, 0);
        die sprintf("statement %d of the substitution has the array %s in its pattern, which perl replaces by its value: write \\\@ for an \@ of the text\n", $index, $array) if defined $array;
        my $value = $second eq "'" ? undef : variable_in($replacement, 1);
        die sprintf("statement %d of the substitution has %s in its replacement, which perl replaces by its value: a replacement may hold \$1 to \$9 and \$& and no other variable; write \\\$ for a \$ and \\\@ for an \@ of the text\n", $index, $value) if defined $value;
        die sprintf("statement %d of the substitution has a pattern into which perl puts the value of a variable, so it is not the text written: write \\\$ for a \$ and \\\@ for an \@ of the text\n", $index) if interpolates($text);
        push @statements, { text => $text, open => $open, close => $close, pattern => $pattern, flags => $flags };
        $at = skip_space($program, $at);
        last if $at >= length $program;
        substr($program, $at, 1) eq ';' or die $not_statements;
        $at = skip_space($program, $at + 1);
    }
    @statements or die "the substitution is empty\n";
    return @statements;
}

sub substituted {
    my ($code, $text) = @_;
    my ($count, $changed, $error) = evaluated($code, $text);
    die "the substitution is no valid perl\n" if $error;
    return ($count || 0, $changed);
}

sub places {
    my ($statement, $text) = @_;
    (my $flags = $statement->{flags}) =~ tr/msixpodualn//cd;
    my ($re, undef, $error) = evaluated("qr$statement->{open}$statement->{pattern}$statement->{close}$flags", '');
    die "the substitution is no valid perl\n" if $error || ref $re ne 'Regexp';
    my (@places, %last_start_at_end);
    my $scan = $text;
    pos($scan) = 0;
    while ($scan =~ /$re/g) {
        my ($from, $to) = ($-[0], $+[0]);
        my $earlier = $last_start_at_end{$to};
        my $same = defined $earlier && substr($scan, $earlier, $from - $earlier) =~ /\A\s*\z/;
        $last_start_at_end{$to} = $from;
        push @places, [$from, $to] unless $same;
        last if $from >= length $scan;
        pos($scan) = $from + 1;
    }
    my ($overlap, $reach) = (0, -1);
    for my $place (@places) {
        $overlap = 1 if $place->[0] < $reach;
        $reach = $place->[1] if $place->[1] > $reach;
    }
    my @lines;
    for my $place (@places) {
        my ($from, $to) = @$place;
        my $lead = substr($scan, $from, $to - $from) =~ /^([\r\n]*)[^\r\n]/ ? length $1 : 0;
        push @lines, $from + $lead >= length $scan ? 'the end of the file' : 1 + (substr($scan, 0, $from + $lead) =~ tr/\n//);
    }
    return ($overlap, @lines);
}

sub listed {
    my @lines = @_;
    return $lines[0] if @lines == 1;
    return join(', ', @lines[0 .. $#lines - 1]) . ' and ' . $lines[-1];
}

sub checked {
    my ($statements, $text, $shown) = @_;
    my $total = @$statements;
    my $index = 0;
    for my $statement (@$statements) {
        $index++;
        my $what = $total == 1 ? 'the substitution' : "statement $index of $total of the substitution";
        my ($overlap, @lines) = places($statement, $text);
        return ('unapplied', "$what did not apply") unless @lines;
        if (@lines > 1 && $statement->{flags} !~ /g/) {
            my $state = $index == 1 ? '' : ' of the file as the statements before it leave it';
            my $fix = $overlap
                ? 'some of them overlap, and the modifier g changes no match that overlaps an earlier one: write its pattern so that it matches one place'
                : 'give the statement the modifier g, which changes every match that does not overlap an earlier one, or write its pattern so that it matches one place';
            return ('places', sprintf('%s matches %d places of %s, at lines %s%s, and would change the first alone: %s', $what, scalar @lines, $shown, listed(@lines), $state, $fix));
        }
        (undef, $text) = substituted($statement->{text}, $text);
    }
    return ('', $text);
}

sub row_places {
    my ($field, $tree, $read) = @_;
    return '-' if $field->[6] eq '-' || $field->[6] =~ /^retired /;
    my $path = $field->[3];
    if ($field->[6] ne 'transitional' && $field->[6] ne 'control' && $field->[0] =~ m{^(.*)/[^/]*$}) {
        $path = "$1/$path";
    }
    return '-' if $path =~ m{^/} || $path =~ m{(?:^|/)\.\.(?:/|$)};
    my $file = "$tree/$path";
    return '-' unless -f $file;
    unless (exists $read->{$file}) {
        open my $in, '<', $file or return '-';
        local $/;
        my $content = <$in>;
        close $in;
        $read->{$file} = defined $content ? $content : '';
    }
    my ($kind, $said) = eval { checked([statements_of($field->[4])], $read->{$file}, $path) };
    if ($@) {
        (my $why = $@) =~ s/\n.*//s;
        return $why;
    }
    return $kind eq 'places' ? $said : '-';
}

unless (caller) {
    my $tree = shift @ARGV;
    defined $tree or die "usage: perl row-places.pl TREE < TABLE\n";
    my %read;
    while (my $line = <STDIN>) {
        chomp $line;
        my @field = split /\t/, $line, -1;
        print $line, "\t", row_places(\@field, $tree, \%read), "\n" or exit 2;
    }
    close STDOUT or exit 2;
    exit 0;
}

1;
