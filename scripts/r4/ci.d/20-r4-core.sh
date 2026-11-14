#!/bin/sh
# jobs: macos windows
# rule: No allow of clippy's disallowed_methods stands outside test code of halvo-coord, halvo-coord-app and halvo-coord-store, by clippy with that lint forbidden over their library, binary and example targets and by a scan of their sources and manifests, each of them keeps the committed clippy.toml with Command::new, spawn, output and status, a normal build of the workspace resolves tauri with neither unstable nor test, the crates that the app links start no child but the two named limits, every test of halvo-coord, and of halvo-coord-app once that crate is in the tree, passes with the committed lockfile and changes no tracked file, and every refusal test of I3.6, I4.8 and I5.11 that the crates hold ran and passed.
set -u
name=20-r4-core

cases='kind_reason kind_class kind_frame second_item items_budget ask_again check_again model_text
answer_twice answer_early answer_page_native timer_allow no_record digest_shown guard_unmatched
guard_digest guard_money guard_releasing_other guard_repeat guard_no_approval_confirm
guard_no_approval_run mixed_request changed_line
two_models two_runs_agent hold_start unrecorded_start second_slot_run auto_needs_person
over_budget queue_full outlives_app stop_tree deleted_workflow interrupted_no_retry
store_not_ended reap_alive after_failed_step7 after_failed_step8 slot_gone agents_unread
check_raised create_again active_tick setup_owed bind_stored gone_outlives_app setup_refused'

mode=measure
case $# in
  0) ;;
  1)
    case $1 in
      --self-test) mode=self_test ;;
      *) mode=usage ;;
    esac
    ;;
  *) mode=usage ;;
esac

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

case $mode in
  usage) fail usage ;;
  *) ;;
esac

case $0 in
  */*) here=${0%/*} ;;
  *) here=. ;;
esac
here=$(cd "$here" 2>/dev/null && pwd) || fail setup
repo=${CI_D_REPO:-}
if [ -z "$repo" ]; then
  case $here in
    *?/scripts/r4/ci.d) repo=${here%/scripts/r4/ci.d} ;;
    *) fail setup ;;
  esac
fi
artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
job=${CI_D_JOB:-windows}
case $job in
  macos | windows) ;;
  *) fail usage ;;
esac

scanner() {
  cat <<'EOF'
use strict;
use warnings;

my ($mode, $listfile, $blobfile, $outfile, @rest) = @ARGV;
my (@paths, @shas, %blob);
{
  open my $l, '<', $listfile or exit 2;
  binmode $l;
  local $/ = "\0";
  while (my $r = <$l>) {
    chomp $r;
    next unless $r =~ /\A(100644|100755) ([0-9a-f]+) 0\t(.+)\z/s;
    push @paths, $3;
    push @shas, $2;
  }
  close $l;
}
if ($mode eq 'ids') {
  print "$_\n" for @shas;
  exit 0;
}
{
  my %bysha;
  open my $b, '<', $blobfile or exit 2;
  binmode $b;
  while (my $h = <$b>) {
    exit 2 unless $h =~ /\A([0-9a-f]+) blob (\d+)\n\z/;
    my ($sha, $len) = ($1, $2);
    my $data = '';
    if ($len > 0) {
      read($b, $data, $len) == $len or exit 2;
    }
    read($b, my $nl, 1);
    $bysha{$sha} = $data;
  }
  close $b;
  for my $i (0 .. $#paths) {
    exit 2 unless exists $bysha{$shas[$i]};
    $blob{$paths[$i]} = $bysha{$shas[$i]};
  }
}

sub blank {
  my ($s) = @_;
  my $o = '';
  my $n = length $s;
  pos($s) = 0;
  while (pos($s) < $n) {
    my $at = pos($s);
    if ($s =~ /\G(?:b|c)?r(#*)"/gc) {
      my $h = $1;
      $s =~ /\G.*?"\Q$h\E/gcs or pos($s) = $n;
    } elsif ($s =~ /\G(?:b|c)?"/gc) {
      $s =~ /\G(?:[^"\\]|\\.)*"/gcs or pos($s) = $n;
    } elsif ($s =~ /\Gb?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,8}\}|.)|[^\\'\n\r])'/gc) {
    } elsif ($s =~ /\G\/\/[^\n]*/gc) {
    } elsif ($s =~ /\G\/\*/gc) {
      my $d = 1;
      while ($d > 0) {
        if ($s =~ /\G\/\*/gc) { $d++ }
        elsif ($s =~ /\G\*\//gc) { $d-- }
        elsif ($s =~ /\G(?:[^\/*]+|.)/gcs) { }
        else { last }
      }
    } elsif ($s =~ /\G(?:'|r#)?[A-Za-z_][A-Za-z0-9_]*|\G[0-9][A-Za-z0-9_]*|\G./gcs) {
      $o .= substr($s, $at, pos($s) - $at);
      next;
    }
    my $t = substr($s, $at, pos($s) - $at);
    $t =~ tr/\n/ /c;
    $o .= $t;
  }
  return $o;
}

sub cfg_test {
  my ($a) = @_;
  $a =~ s/\s+//g;
  return 0 unless $a =~ /\A#!?\[cfg\((.*)\)\]\z/s;
  my $p = $1;
  return 1 if $p eq 'test';
  return 0 unless $p =~ /\Aall\((.*)\)\z/s;
  my ($in, $d, $cur, @args) = ($1, 0, '');
  for my $ch (split //, $in) {
    if ($ch eq ',' && $d == 0) { push @args, $cur; $cur = ''; next }
    $d++ if $ch eq '(';
    $d-- if $ch eq ')';
    $cur .= $ch;
  }
  push @args, $cur;
  return scalar grep { $_ eq 'test' } @args;
}

sub analyse {
  my ($raw) = @_;
  my $c = blank($raw);
  my (@stack, @regions, @decls);
  my ($pend, $pdepth, $pstart, $ptest, $ppath) = (0, 0, 0, 0, undef);
  my $filetest = 0;
  my $end_pending = sub {
    my ($at) = @_;
    push @regions, [$pstart, $at] if $ptest;
    ($pend, $ptest, $ppath) = (0, 0, undef);
  };
  my $ctx = sub { @stack ? $stack[-1]{test} : $filetest };
  my $n = length $c;
  pos($c) = 0;
  while (pos($c) < $n) {
    my $at = pos($c);
    if ($c =~ /\G#(!?)\s*\[/gc) {
      my $inner = $1;
      my $d = 1;
      while ($d > 0 && $c =~ /\G(?:[^\[\]]+|\[|\])/gc) {
        my $t = substr($c, pos($c) - 1, 1);
        $d++ if $t eq '[';
        $d-- if $t eq ']';
      }
      my $text = substr($c, $at, pos($c) - $at);
      my $istest = cfg_test($text);
      if ($inner) {
        if ($istest) {
          if (@stack) {
            $stack[-1]{test} = 1;
            $stack[-1]{rstart} = $stack[-1]{pos} unless defined $stack[-1]{rstart};
          } else {
            $filetest = 1;
          }
        }
        next;
      }
      if (!$pend || $pdepth != @stack) {
        ($pend, $pdepth, $pstart, $ptest, $ppath) = (1, scalar @stack, $at, 0, []);
      }
      $ptest = 1 if $istest;
      my $rawattr = substr($raw, $at, pos($c) - $at);
      if ($rawattr =~ /\A#\s*\[\s*(?:path\s*=|cfg_attr\s*\()/) {
        push @$ppath, $rawattr =~ /\bpath\s*=\s*"([^"]*)"/g;
      }
      next;
    }
    if ($c =~ /\Gmod\s+(?:r#)?([A-Za-z_][A-Za-z0-9_]*)\s*([;{])/gc) {
      my ($name, $end) = ($1, $2);
      my $here = $pend && $pdepth == @stack;
      my $test = $ctx->() || ($here && $ptest) ? 1 : 0;
      my @inline = map { $_->{mod} } grep { defined $_->{mod} } @stack;
      if ($end eq ';') {
        push @decls, [$name, $test, $here ? $ppath : undef, [@inline], $at];
        $end_pending->(pos($c) - 1) if $here;
      } else {
        my $e = { ch => '}', pos => pos($c) - 1, test => $test, mod => $name };
        if ($here) {
          $e->{rstart} = $pstart if $ptest;
          ($pend, $ptest, $ppath) = (0, 0, undef);
        }
        push @stack, $e;
      }
      next;
    }
    if ($c =~ /\G([{(\[])/gc) {
      my $ch = $1;
      my $e = { ch => ($ch eq '{' ? '}' : $ch eq '(' ? ')' : ']'), pos => $at, test => $ctx->() };
      if ($ch eq '{' && $pend && $pdepth == @stack) {
        if ($ptest) { $e->{test} = 1; $e->{rstart} = $pstart }
        ($pend, $ptest, $ppath) = (0, 0, undef);
      }
      push @stack, $e;
      next;
    }
    if ($c =~ /\G([})\]])/gc) {
      my $e = pop @stack;
      push @regions, [$e->{rstart}, $at] if $e && defined $e->{rstart};
      $end_pending->($at) if $pend && $pdepth > @stack;
      next;
    }
    if ($c =~ /\G;/gc) {
      $end_pending->($at) if $pend && $pdepth == @stack;
      next;
    }
    $c =~ /\G(?:'?[A-Za-z_][A-Za-z0-9_]*|[0-9][A-Za-z0-9_]*|\s+|.)/gcs;
  }
  push @regions, [0, $n] if $filetest;
  return { code => $c, regions => \@regions, decls => \@decls };
}

my %info;
sub info {
  my ($p) = @_;
  $info{$p} //= analyse($blob{$p});
  return $info{$p};
}

sub norm {
  my @out;
  for my $part (split m{/}, $_[0]) {
    next if $part eq '' || $part eq '.';
    if ($part eq '..') { pop @out } else { push @out, $part }
  }
  return join '/', @out;
}

sub children {
  my ($f) = @_;
  my ($dir, $base) = $f =~ m{\A(.*)/([^/]+)\.rs\z} ? ($1, $2) : ('', '');
  my $modrs = $base =~ /\A(?:mod|lib|main|build)\z/ || $f =~ m{/src/bin/[^/]+\.rs\z};
  my $root = $modrs ? $dir : "$dir/$base";
  my @out;
  for my $d (@{ info($f)->{decls} }) {
    my ($name, $test, $path, $inline) = @$d;
    my $b = join '/', $root, @$inline;
    my @cand = ("$b/$name.rs", "$b/$name/mod.rs");
    push @cand, map { norm(@$inline ? "$b/$_" : "$dir/$_") } @{ $path // [] };
    my @found = grep { exists $blob{$_} } @cand;
    push @out, [undef, $test, "$f:" . line_of($f, $d->[4])] unless @found;
    push @out, map { [$_, $test, "$f:" . line_of($f, $d->[4])] } @found;
  }
  return @out;
}

sub line_of {
  my ($f, $off) = @_;
  return 1 + (substr($blob{$f}, 0, $off) =~ tr/\n//);
}

sub walk {
  my (@roots) = @_;
  my (%kind, @queue, @unresolved);
  for my $r (@roots) { $kind{$r} = 'product'; push @queue, $r }
  while (@queue) {
    my $f = shift @queue;
    for my $ch (children($f)) {
      my ($child, $test, $where) = @$ch;
      my $k = ($kind{$f} eq 'test' || $test) ? 'test' : 'product';
      if (!defined $child) {
        push @unresolved, $where if $k eq 'product';
        next;
      }
      next if defined $kind{$child} && ($kind{$child} eq 'product' || $k eq 'test');
      $kind{$child} = $k;
      push @queue, $child;
    }
  }
  return (\%kind, \@unresolved);
}

sub in_test {
  my ($f, $off) = @_;
  for my $r (@{ info($f)->{regions} }) {
    return 1 if $off >= $r->[0] && $off <= $r->[1];
  }
  return 0;
}

sub hits {
  my ($f, $re) = @_;
  my $c = info($f)->{code};
  my (%seen, @out);
  while ($c =~ /$re/g) {
    my $off = $-[0];
    next if in_test($f, $off);
    my $line = line_of($f, $off);
    next if $seen{$line}++;
    push @out, [$line, $&];
  }
  return @out;
}

open my $out, '>', $outfile or exit 2;
my $found = 0;

if ($mode eq 'allow') {
  my ($expected, @crates) = @rest;
  my $want;
  {
    open my $e, '<', $expected or exit 2;
    binmode $e;
    local $/;
    $want = <$e>;
    close $e;
  }
  my %has = map { $_ => 1 } keys %blob;
  my %allowed = map { $_ => 1 } qw(test feature target_os windows unix any all not);
  my @find;
  my $note = sub { push @find, join(' ', 'allow', @_) };
  for my $p (grep { m{(?:\A|/)\.cargo/config(?:\.toml)?\z} } sort keys %has) { $note->('cargo-config', $p) }
  for my $p (grep { m{(?:\A|/)\.?clippy\.toml\z} } sort keys %has) {
    my $p2 = $p;
    $note->('clippy-config', $p) unless grep { $p2 eq "$_/clippy.toml" } @crates;
  }
  my ($crates, $files) = (0, 0);
  for my $cr (@crates) {
    next unless $has{"$cr/Cargo.toml"};
    $crates++;
    my $toml = $blob{"$cr/clippy.toml"};
    $note->('clippy-config', "$cr/clippy.toml", 'differs') if !defined $toml || $toml ne $want;
    for my $m (grep { m{\A\Q$cr\E/.+/Cargo\.toml\z} } sort keys %has) { $note->('package', $m) }
    my $i = 0;
    for my $line (split /\n/, $blob{"$cr/Cargo.toml"}) {
      $i++;
      $note->('build', "$cr/Cargo.toml:$i", 'key') if $line =~ /\A\s*build\s*=\s*(.*?)\s*\z/ && $1 ne '"build.rs"';
    }
    for my $f (grep { m{\A\Q$cr\E/.+\.rs\z} && !m{\A\Q$cr\E/tests/} } sort keys %has) {
      $files++;
      my $raw = $blob{$f};
      while ($raw =~ /rustc-cfg/g) { $note->('build', "$f:" . line_of($f, $-[0]), 'rustc-cfg') }
      my $c = info($f)->{code};
      my $n = length $c;
      while ($c =~ /\bcfg(_attr)?\s*!?\s*\(/g) {
        my $attr = defined $1;
        my $at = $-[0];
        my $open = pos($c) - 1;
        my ($d, $k) = (0, $open);
        for (; $k < $n; $k++) {
          my $ch = substr($c, $k, 1);
          $d++ if $ch eq '(';
          if ($ch eq ')') { $d--; last if $d == 0 }
        }
        my $pred = substr($c, $open + 1, $k - $open - 1);
        if ($attr) {
          my ($dd, $cut) = (0, length $pred);
          for my $j (0 .. length($pred) - 1) {
            my $ch = substr($pred, $j, 1);
            $dd++ if $ch eq '(';
            $dd-- if $ch eq ')';
            if ($ch eq ',' && $dd == 0) { $cut = $j; last }
          }
          $pred = substr($pred, 0, $cut);
        }
        my $where = "$f:" . line_of($f, $at);
        $note->('cfg', $where, 'dollar') if $pred =~ /\$/;
        my %seen;
        for my $id ($pred =~ /(?:r#)?([A-Za-z_][A-Za-z0-9_]*)/g) {
          $note->('cfg', $where, $id) unless $allowed{$id} || $seen{$id}++;
        }
        pos($c) = $open + 1;
      }
    }
  }
  my $fl = 'RUST' . 'FLAGS';
  for my $v (sort keys %ENV) {
    next unless $v =~ /\A(?:|CARGO_ENCODED_|CARGO_BUILD_|CARGO_TARGET_[A-Z0-9_]+_)\Q$fl\E\z/ || $v eq 'CLIPPY_ARGS';
    my @tok = split /[\s\x1f]+|__CLIPPY_HACKERY__/, $ENV{$v};
    $note->('flags', $v) if grep { /\A-A/ || /\A--allow\b/ || /cap-lints/ || /\A--cfg\b/ } @tok;
  }
  for my $l (@find) { print "$l\n"; print $out "$l\n" }
  $found = @find;
  printf "allow crates=%d files=%d found=%d\n", $crates, $files, $found;
} elsif ($mode eq 'starts') {
  my ($app, $linkedfile) = @rest;
  my %linked;
  open my $lf, '<', $linkedfile or exit 2;
  while (<$lf>) { s/\r?\n\z//; $linked{$1} = 1 if /\A(\S+)/ }
  close $lf;
  my %dir;
  for my $m (grep { m{\A(?:src-tauri|crates/[^/]+)/Cargo\.toml\z} } keys %blob) {
    my ($d) = $m =~ m{\A(.*)/Cargo\.toml\z};
    my $pkg = 0;
    for my $line (split /\n/, $blob{$m}) {
      if ($line =~ /\A\s*\[\s*([^\]]*?)\s*\]/) { $pkg = $1 eq 'package'; next }
      if ($pkg && $line =~ /\A\s*name\s*=\s*"([^"]+)"/) { $dir{$1} = $d; last }
    }
  }
  my @limits = (['crates/halvo-control/src/onboarding.rs', '/usr/bin/open'],
                ['crates/halvo-capture/src/consent/onboarding.rs', '/usr/bin/sw_vers']);
  my %used;
  my ($crates, $files, $all, $lim, $other) = (0, 0, 0, 0, 0);
  my $re = qr/\bCommand\s*::\s*new\b|\bposix_spawnp?\b|\b(?:libc|unistd)\s*::\s*(?:fork|vfork|exec[lv]\w*|system|popen)\b|\bCreateProcess\w*|\bShellExecute\w*|\bWinExec\b|\bNSTask\b/;
  my @unres;
  for my $name (sort keys %linked) {
    next unless defined $dir{$name};
    $crates++;
    my $d = $dir{$name};
    my @roots = grep { exists $blob{$_} } ("$d/src/lib.rs", $name eq $app ? "$d/src/main.rs" : ());
    my ($kind, $unresolved) = walk(@roots);
    push @unres, @$unresolved;
    for my $f (sort grep { $kind->{$_} eq 'product' } keys %$kind) {
      $files++;
      my @lines = split /\n/, $blob{$f}, -1;
      for my $h (hits($f, $re)) {
        $all++;
        my $text = $lines[$h->[0] - 1];
        my ($l) = grep { !$used{$_} && $limits[$_][0] eq $f && $text =~ /\bCommand\s*::\s*new\s*\(\s*"\Q$limits[$_][1]\E"\s*\)/ } 0 .. $#limits;
        if (defined $l) {
          $used{$l} = 1;
          $lim++;
          print $out "start limit $f:$h->[0]\n";
        } else {
          $other++;
          print $out "start other $f:$h->[0]\n";
        }
      }
    }
  }
  for my $u (@unres) { print $out "start unresolved $u\n" }
  $found = $other;
  printf "starts crates=%d files=%d hits=%d limits=%d other=%d unresolved=%d\n",
    $crates, $files, $all, $lim, $other, scalar @unres;
} else {
  exit 2;
}
close $out or exit 2;
exit($found ? 1 : 0);
EOF
}

lint_config() {
  cat <<'EOF'
disallowed-methods = [
  { path = "std::process::Command::new", reason = "no code of the interface run's crates starts a process; a runner is reached through the supervisor library alone (contracts I10.1, ruling RD-67 2.2)" },
  { path = "std::process::Command::spawn", reason = "no code of the interface run's crates starts a process; a runner is reached through the supervisor library alone (contracts I10.1, ruling RD-67 2.2)" },
  { path = "std::process::Command::output", reason = "no code of the interface run's crates starts a process; a runner is reached through the supervisor library alone (contracts I10.1, ruling RD-67 2.2)" },
  { path = "std::process::Command::status", reason = "no code of the interface run's crates starts a process; a runner is reached through the supervisor library alone (contracts I10.1, ruling RD-67 2.2)" },
]
EOF
}

blobs() {
  label=$1
  shift
  git ls-files -s -z -- "$@" >"$artifacts/$label.list" || fail setup
  perl "$scan" ids "$artifacts/$label.list" >"$artifacts/$label.ids" || fail setup
  git cat-file --batch <"$artifacts/$label.ids" >"$artifacts/$label.blobs" || fail setup
}

lint_allows() {
  if [ -n "${CLIPPY_CONF_DIR:-}" ]; then
    printf 'allow CLIPPY_CONF_DIR is set\n'
    fail allow
  fi
  lint_config >"$artifacts/clippy.toml.expected" || fail setup
  blobs allow crates/halvo-coord crates/halvo-coord-app crates/halvo-coord-store .cargo crates/.cargo \
    clippy.toml .clippy.toml crates/clippy.toml crates/.clippy.toml
  perl "$scan" allow "$artifacts/allow.list" "$artifacts/allow.blobs" "$artifacts/allow.txt" \
    "$artifacts/clippy.toml.expected" crates/halvo-coord crates/halvo-coord-app crates/halvo-coord-store
  code=$?
  case $code in
    0) ;;
    1) fail allow ;;
    *) fail setup ;;
  esac
}

forbid_run() {
  set --
  for crate in halvo-coord halvo-coord-app halvo-coord-store; do
    if [ -f "crates/$crate/Cargo.toml" ]; then
      set -- "$@" -p "$crate"
    fi
  done
  [ "$#" -gt 0 ] || return 0
  cargo clippy "$@" --lib --bins --examples --all-features --locked -- -F clippy::disallowed_methods >"$artifacts/clippy-forbid.txt" 2>&1
  code=$?
  printf 'allow clippy with the lint forbidden: exit=%s\n' "$code"
  [ "$code" -eq 0 ] || fail allow
}

tauri_features() {
  cargo tree -e normal --workspace --target all -i tauri --depth 0 --format '{f}' --locked \
    >"$artifacts/tauri-features.txt" 2>"$artifacts/tauri-tree.txt"
  code=$?
  [ "$code" -eq 0 ] || fail tree
  features=$(tr '\r\n' ',,' <"$artifacts/tauri-features.txt") || fail setup
  count=0
  unstable=no
  testf=no
  old_ifs=$IFS
  IFS=,
  for feature in $features; do
    [ -n "$feature" ] || continue
    count=$((count + 1))
    case $feature in
      unstable) unstable=yes ;;
      test) testf=yes ;;
      *) ;;
    esac
  done
  IFS=$old_ifs
  printf 'tauri normal features=%s unstable=%s test=%s\n' "$count" "$unstable" "$testf"
  if [ "$unstable" = yes ] || [ "$testf" = yes ]; then
    fail tauri
  fi
}

child_starts() {
  cargo tree -e normal --target all -p halvo --prefix none --format '{p}' --locked \
    >"$artifacts/linked.txt" 2>"$artifacts/linked-tree.txt"
  code=$?
  [ "$code" -eq 0 ] || fail tree
  blobs starts 'src-tauri/*.rs' 'crates/*.rs' src-tauri/Cargo.toml 'crates/*/Cargo.toml'
  perl "$scan" starts "$artifacts/starts.list" "$artifacts/starts.blobs" "$artifacts/starts.txt" \
    halvo "$artifacts/linked.txt"
  code=$?
  case $code in
    0) ;;
    1) fail start ;;
    *) fail setup ;;
  esac
}

measure() {
  cd "$repo" 2>/dev/null || fail setup
  [ -d "$artifacts" ] || mkdir -p "$artifacts" 2>/dev/null || fail setup
  log=$artifacts/cargo-test.txt
  lines=$artifacts/cargo-test-lines.txt
  sources=$artifacts/refusal-sources.txt
  scan=$artifacts/scan.pl
  scanner >"$scan" || fail setup
  if [ "$job" = macos ]; then
    forbid_run
    printf 'PASS %s\n' "$name"
    return 0
  fi
  git diff --binary >"$artifacts/tracked-before.txt" || fail setup
  lint_allows
  forbid_run
  tauri_features
  child_starts
  : >"$log" 2>/dev/null || fail setup
  app=no
  if [ -f crates/halvo-coord-app/Cargo.toml ]; then
    app=yes
    cargo test -p halvo-coord -p halvo-coord-app --locked >"$log" 2>&1
  else
    cargo test -p halvo-coord --locked >"$log" 2>&1
  fi
  code=$?
  printf 'cargo test exit=%s halvo-coord-app=%s\n' "$code" "$app"
  [ "$code" -eq 0 ] || fail cargo
  git diff --binary >"$artifacts/tracked-after.txt" || fail setup
  cmp -s "$artifacts/tracked-before.txt" "$artifacts/tracked-after.txt" || fail changed
  tr -d '\r' <"$log" >"$lines" || fail setup
  git -C "$repo" grep -h -o -E 'fn refusal_[a-z0-9_]+' -- crates/halvo-coord crates/halvo-coord-app >"$sources"
  code=$?
  case $code in
    0 | 1) ;;
    *) fail setup ;;
  esac
  listed=0
  held=0
  passed=0
  missing=0
  for c in $cases; do
    listed=$((listed + 1))
    in_source=no
    if grep -q -x "fn refusal_$c" "$sources"; then
      in_source=yes
      held=$((held + 1))
    fi
    if grep -q -x "test refusal_$c \.\.\. ok" "$lines"; then
      passed=$((passed + 1))
    elif [ "$in_source" = yes ]; then
      missing=$((missing + 1))
    fi
  done
  printf 'refusal cases listed=%s in_sources=%s passed=%s missing=%s\n' "$listed" "$held" "$passed" "$missing"
  printf 'refusals=%s\n' "$passed"
  printf 'refusals=%s\n' "$passed" >"$artifacts/result.txt" || fail setup
  [ "$missing" -eq 0 ] || fail missing
  printf 'PASS %s\n' "$name"
}

check() {
  label=$1
  want=$2
  text=$3
  shift 3
  out=$(
    PATH=$work/bin:$PATH
    export PATH STUB_OUT STUB_EXIT STUB_ARGS STUB_CLIPPY_EXIT STUB_TAURI STUB_TAURI_EXIT STUB_LINKED_EXIT STUB_LINKED STUB_WRITE
    "$@" 2>&1
  )
  code=$?
  last=${out##*"
"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$label" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $label"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_EXIT=0
  STUB_CLIPPY_EXIT=0
  STUB_TAURI_EXIT=0
  STUB_LINKED_EXIT=0
  STUB_TAURI=default,wry
  STUB_WRITE=
}

put() {
  mkdir -p "${1%/*}" || exit 2
  printf '%b' "$2" >"$1" || exit 2
}

crate() {
  put "$1/Cargo.toml" "[package]\nname = \"${1##*/}\"\nversion = \"0.0.0\"\n"
  lint_config >"$1/clippy.toml" || exit 2
}

fixture() {
  dir=$work/$1
  mkdir -p "$dir/crates/halvo-coord/tests" || exit 2
  printf '#[test]\nfn refusal_two_models() {}\n#[test]\nfn refusal_queue_full() {}\n#[test]\nfn refusal_not_a_case() {}\n' >"$dir/crates/halvo-coord/tests/refusal.rs" || exit 2
  crate "$dir/crates/halvo-coord"
  put "$dir/crates/halvo-coord/src/lib.rs" '//! No #[allow(clippy::disallowed_methods)] here.\npub mod words;\n#[cfg(test)]\nmod tests;\n#[cfg(all(test, unix))]\nmod more {\n    const B: char = '"'"'}'"'"';\n    const S: &str = "}{";\n    #[allow(clippy::disallowed_methods)]\n    fn f() {}\n}\npub fn after() -> &'"'"'static str {\n    r#"x"y #[allow(clippy::disallowed_methods)] }"#\n}\n'
  put "$dir/crates/halvo-coord/src/words.rs" 'pub fn w() {}\n'
  put "$dir/crates/halvo-coord/src/tests.rs" '#![cfg(test)]\nmod deep;\n#[allow(clippy::disallowed_methods)]\nfn t() {}\n'
  put "$dir/crates/halvo-coord/src/tests/deep.rs" '#![cfg(test)]\n#[expect(clippy::disallowed_methods)]\nfn d() {}\n'
  put "$dir/crates/halvo-coord/tests/words_same.rs" '#[expect(clippy::disallowed_methods, reason = "fixture")]\nfn s() {}\n'
  crate "$dir/crates/halvo-coord-store"
  put "$dir/crates/halvo-coord-store/src/lib.rs" '#[cfg(all(any(test, unix), not(windows), feature = "x", target_os = "macos"))]\npub fn s() {}\n#[cfg_attr(feature = "y", derive(Debug))]\npub struct T;\nmacro_rules! m {\n    (\0044(#[\0044a:meta])* \0044t:ident) => {\n        \0044(#[\0044a])* pub struct \0044t;\n    };\n}\nm!(#[cfg(test)] U);\npub const C: bool = cfg!(feature = "z");\n// #[cfg(not(clippy))]\npub const D: &str = "#[cfg(debug_assertions)]";\n'
  put "$dir/crates/halvo-coord-store/build.rs" 'fn main() {\n    println!("cargo:rerun-if-changed=build.rs");\n    println!("cargo:rustc-check-cfg=cfg(x)");\n}\n'
  put "$dir/crates/halvo-coord-store/tests/odd.rs" '#[cfg(not(clippy))]\nfn t() {}\n'
  put "$dir/Cargo.toml" '[workspace]\nmembers = ["crates/*", "src-tauri"]\n'
  put "$dir/crates/halvo-web/clippy.toml" 'disallowed-methods = []\n'
  put "$dir/tools/r3-trials/profile/clippy.toml" 'disallowed-methods = []\n'
  put "$dir/tools/r3-trials/profile/.cargo/config.toml" '[build]\njobs = 1\n'
  if [ "${2:-}" = app ]; then
    crate "$dir/crates/halvo-coord-app"
    put "$dir/crates/halvo-coord-app/src/lib.rs" 'pub fn a() {}\n'
  fi
  put "$dir/src-tauri/Cargo.toml" '[package]\nname = "halvo"\n'
  put "$dir/src-tauri/src/main.rs" 'mod agents;\nfn main() {}\n'
  put "$dir/src-tauri/src/agents.rs" '#[cfg(test)]\nmod tests {\n    fn t() { let _ = std::process::Command::new("/bin/ls"); }\n}\n'
  put "$dir/crates/halvo-control/Cargo.toml" '[package]\nname = "halvo-control"\n'
  put "$dir/crates/halvo-control/src/lib.rs" '#[cfg(feature = "absent")]\nmod absent;\npub mod onboarding;\n#[cfg(test)]\nmod testing;\n'
  put "$dir/crates/halvo-control/src/onboarding.rs" '// std::process::Command::new("/bin/ls")\npub fn open() {\n    let _ = std::process::Command::new("/usr/bin/open").status();\n}\n'
  put "$dir/crates/halvo-control/src/testing.rs" 'pub fn t() { let _ = std::process::Command::new("/bin/ls"); }\n'
  put "$dir/crates/halvo-control/tests/start.rs" 'fn t() { let _ = std::process::Command::new("/bin/ls"); }\n'
  put "$dir/crates/halvo-other/Cargo.toml" '[package]\nname = "halvo-other"\n'
  put "$dir/crates/halvo-other/src/lib.rs" 'pub fn t() { let _ = std::process::Command::new("/bin/ls"); }\n'
  git -C "$dir" init -q >/dev/null 2>&1 || exit 2
  git -C "$dir" add . >/dev/null 2>&1 || exit 2
}

variant() {
  fixture "$1" app
  put "$work/$1/$2" "$3"
  git -C "$work/$1" add -A . >/dev/null 2>&1 || exit 2
}

output() {
  STUB_OUT=$work/out.$1
  shift
  : >"$STUB_OUT" || exit 2
  for line in "$@"; do
    printf '%s\r\n' "$line" >>"$STUB_OUT" || exit 2
  done
}

run_one() {
  CI_D_REPO=$work/one CI_D_ARTIFACTS=$work/artifacts-one CI_D_JOB=windows sh "$here/$name.sh"
}

run_in() {
  CI_D_REPO=$work/$1 CI_D_ARTIFACTS=$work/artifacts-$1 CI_D_JOB=windows sh "$here/$name.sh"
}

run_mac() {
  CI_D_REPO=$work/$1 CI_D_ARTIFACTS=$work/artifacts-macos-$1 CI_D_JOB=macos sh "$here/$name.sh"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  trap 'rm -rf "$work"' EXIT
  trap 'rm -rf "$work"; exit 2' HUP INT TERM
  mkdir -p "$work/bin" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_ARGS"
case " $* " in
  *' --locked '*) ;;
  *) exit 97 ;;
esac
want=clippy
for c in halvo-coord halvo-coord-app halvo-coord-store; do
  if [ -f "crates/$c/Cargo.toml" ]; then
    want="$want -p $c"
  fi
done
want="$want --lib --bins --examples --all-features --locked -- -F clippy::disallowed_methods"
if [ "$CI_D_JOB" = macos ] && [ "$*" != "$want" ]; then
  exit 92
fi
case $* in
  "$want")
    exit "$STUB_CLIPPY_EXIT"
    ;;
  clippy*) exit 93 ;;
  'tree -e normal --workspace --target all -i tauri --depth 0 --format {f} --locked')
    printf '%s\n' "$STUB_TAURI"
    exit "$STUB_TAURI_EXIT"
    ;;
  'tree -e normal --target all -p halvo --prefix none --format {p} --locked')
    cat "$STUB_LINKED"
    exit "$STUB_LINKED_EXIT"
    ;;
  tree*) exit 94 ;;
esac
if [ -f crates/halvo-coord-app/Cargo.toml ]; then
  [ "$*" = 'test -p halvo-coord -p halvo-coord-app --locked' ] || exit 96
else
  [ "$*" = 'test -p halvo-coord --locked' ] || exit 95
fi
[ -z "$STUB_WRITE" ] || printf 'changed\n' >>"$STUB_WRITE"
cat "$STUB_OUT"
exit "$STUB_EXIT"
EOF
  chmod +x "$work/bin/cargo" || exit 2
  STUB_ARGS=$work/args
  STUB_EXIT=0
  STUB_CLIPPY_EXIT=0
  STUB_TAURI_EXIT=0
  STUB_LINKED_EXIT=0
  STUB_TAURI=default,wry
  STUB_WRITE=
  STUB_LINKED=$work/linked.txt
  printf 'halvo v0.0.0 (/fixture/src-tauri)\nhalvo-control v0.0.0 (/fixture/crates/halvo-control)\nhalvo-control v0.0.0 (/fixture/crates/halvo-control) (*)\nserde v1.0.0\n' >"$STUB_LINKED" || exit 2
  rejected=0
  wrong=
  fixture one
  fixture app app
  : >"$work/plain-file" || exit 2
  ok_both='test refusal_two_models ... ok'
  ok_queue='test refusal_queue_full ... ok'
  ok_other='test refusal_not_a_case ... ok'
  done_line='test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out'

  output pass "$ok_both" "$ok_queue" "$ok_other" "$done_line"
  check pass PASS 'refusals=2' run_one
  check app-crate-in-the-tree PASS 'halvo-coord-app=yes' \
    env -u CI_D_JOB CI_D_REPO="$work/app" CI_D_ARTIFACTS="$work/artifacts-app" sh "$here/$name.sh"
  check macos-job-runs-the-forbid-run-alone PASS 'allow clippy with the lint forbidden: exit=0' run_mac app
  STUB_CLIPPY_EXIT=101
  check macos-job-forbid-run-fails FAIL "FAIL $name (allow)" run_mac app
  check a-job-that-is-neither FAIL "FAIL $name (usage)" \
    env CI_D_JOB=linux CI_D_REPO="$work/app" CI_D_ARTIFACTS="$work/artifacts-linux" sh "$here/$name.sh"
  if [ "$job" = macos ]; then
    finish
  fi
  check text-rules-pass-their-controls PASS 'allow crates=2 files=' run_one
  check starts-in-tests-and-unlinked-crates-only PASS 'starts crates=2 files=4 hits=1 limits=1 other=0 unresolved=1' run_one
  STUB_TAURI=default,test-util,wry,unstable-x
  check tauri-features-that-only-hold-the-words PASS 'tauri normal features=4 unstable=no test=no' run_one
  check usage FAIL "FAIL $name (usage)" sh "$here/$name.sh" --bogus
  check setup FAIL "FAIL $name (setup)" \
    env CI_D_REPO="$work/one" CI_D_ARTIFACTS="$work/plain-file/artifacts" sh "$here/$name.sh"
  STUB_EXIT=101
  check cargo FAIL "FAIL $name (cargo)" run_one
  output ignored "$ok_both" 'test refusal_queue_full ... ignored' "$done_line"
  check a-refusal-test-that-did-not-run FAIL "FAIL $name (missing)" run_one
  output absent "$ok_queue" "$done_line"
  check a-refusal-test-that-cargo-did-not-list FAIL "FAIL $name (missing)" run_one
  output failed "$ok_both" 'test refusal_queue_full ... FAILED' "$done_line"
  STUB_EXIT=101
  check a-refusal-test-that-failed FAIL "FAIL $name (cargo)" run_one
  output pass "$ok_both" "$ok_queue" "$ok_other" "$done_line"

  variant allow-config .cargo/config.toml '[env]\nCLIPPY_CONF_DIR = "x"\n'
  check an-allow-in-the-cargo-configuration FAIL "FAIL $name (allow)" run_in allow-config
  variant allow-short crates/halvo-coord-app/clippy.toml 'disallowed-methods = [\n{ path = "std::process::Command::new" },\n]\n'
  check a-clippy-toml-without-spawn FAIL "FAIL $name (allow)" run_in allow-short
  variant allow-commented crates/halvo-coord/clippy.toml '# disallowed-methods = []\n'
  check a-clippy-toml-that-differs-from-the-committed-one FAIL "FAIL $name (allow)" run_in allow-commented
  variant allow-crate-config crates/halvo-coord/.cargo/config.toml '[env]\nX = "1"\n'
  check a-cargo-configuration-in-a-crate FAIL "FAIL $name (allow)" run_in allow-crate-config
  check a-clippy-configuration-directory-in-the-environment FAIL "FAIL $name (allow)" \
    env CLIPPY_CONF_DIR="$work" CI_D_REPO="$work/one" CI_D_ARTIFACTS="$work/artifacts-env" sh "$here/$name.sh"
  STUB_CLIPPY_EXIT=101
  check clippy-with-the-lint-forbidden-fails FAIL "FAIL $name (allow)" run_one
  variant cfg-clippy crates/halvo-coord/src/words.rs '#[cfg(not(clippy))]\npub fn w() {}\n'
  check a-cfg-that-names-clippy FAIL "allow cfg crates/halvo-coord/src/words.rs:1 clippy" run_in cfg-clippy
  variant cfg-debug crates/halvo-coord-app/src/lib.rs 'pub fn a() {\n    #[cfg(not(debug_assertions))]\n    let _ = 1;\n}\n'
  check a-cfg-that-names-debug-assertions FAIL "allow cfg crates/halvo-coord-app/src/lib.rs:2 debug_assertions" run_in cfg-debug
  variant cfg-attr-clippy crates/halvo-coord/src/words.rs '#[cfg_attr(clippy, path = "x.rs")]\npub mod w;\n'
  check a-cfg-attr-that-names-clippy FAIL "allow cfg crates/halvo-coord/src/words.rs:1 clippy" run_in cfg-attr-clippy
  variant cfg-dollar crates/halvo-coord/src/words.rs 'macro_rules! g {\n    (\0044c:ident) => {\n        #[cfg(not(\0044c))]\n        pub fn w() {}\n    };\n}\ng!(clippy);\n'
  check a-cfg-that-a-macro-pastes FAIL "allow cfg crates/halvo-coord/src/words.rs:3 dollar" run_in cfg-dollar
  variant build-cfg crates/halvo-coord-store/build.rs 'fn main() {\n    println!("cargo:rustc-cfg=x");\n}\n'
  check a-build-script-that-sets-a-cfg FAIL "allow build crates/halvo-coord-store/build.rs:2 rustc-cfg" run_in build-cfg
  variant build-key crates/halvo-coord/Cargo.toml '[package]\nname = "halvo-coord"\nbuild = "gen.rs"\n'
  check a-build-script-under-another-name FAIL "allow build crates/halvo-coord/Cargo.toml:3 key" run_in build-key
  variant toml-sub crates/halvo-coord/spawn/clippy.toml 'disallowed-methods = []\n'
  check a-clippy-toml-below-a-crate FAIL "allow clippy-config crates/halvo-coord/spawn/clippy.toml" run_in toml-sub
  variant toml-root clippy.toml 'disallowed-methods = []\n'
  check a-clippy-toml-at-the-root FAIL "allow clippy-config clippy.toml" run_in toml-root
  variant toml-crates crates/.clippy.toml 'disallowed-methods = []\n'
  check a-clippy-toml-in-crates FAIL "allow clippy-config crates/.clippy.toml" run_in toml-crates
  variant package-sub crates/halvo-coord/spawn/Cargo.toml '[package]\nname = "spawn"\n'
  check a-package-inside-a-crate FAIL "allow package crates/halvo-coord/spawn/Cargo.toml" run_in package-sub
  variant config-legacy crates/.cargo/config '[env]\nX = "1"\n'
  check a-cargo-configuration-without-its-suffix FAIL "allow cargo-config crates/.cargo/config" run_in config-legacy
  flags_var=RUST
  flags_var=${flags_var}FLAGS
  unit=$(printf '\037') || exit 2
  check a-flags-variable-that-caps-lints FAIL "allow flags CARGO_ENCODED_$flags_var" \
    env "CARGO_ENCODED_$flags_var=-Dwarnings$unit--cap-lints=warn" CI_D_REPO="$work/one" CI_D_ARTIFACTS="$work/artifacts-flags-1" CI_D_JOB=windows sh "$here/$name.sh"
  check a-flags-variable-with-an-allow FAIL "allow flags $flags_var" \
    env "$flags_var=-Aclippy::all" CI_D_REPO="$work/one" CI_D_ARTIFACTS="$work/artifacts-flags-2" CI_D_JOB=windows sh "$here/$name.sh"
  check a-flags-variable-with-a-cfg FAIL "allow flags CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_$flags_var" \
    env "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_$flags_var=--cfg=x" CI_D_REPO="$work/one" CI_D_ARTIFACTS="$work/artifacts-flags-3" CI_D_JOB=windows sh "$here/$name.sh"
  check a-clippy-args-variable-with-an-allow FAIL "allow flags CLIPPY_ARGS" \
    env "CLIPPY_ARGS=--allow=clippy::style" CI_D_REPO="$work/one" CI_D_ARTIFACTS="$work/artifacts-flags-4" CI_D_JOB=windows sh "$here/$name.sh"
  check a-flags-variable-that-holds-no-refused-flag PASS 'allow crates=2' \
    env "$flags_var=-Dwarnings" CI_D_REPO="$work/one" CI_D_ARTIFACTS="$work/artifacts-flags-5" CI_D_JOB=windows sh "$here/$name.sh"
  variant allow-dot crates/halvo-coord/.clippy.toml 'disallowed-methods = []\n'
  check a-second-clippy-toml-that-clippy-reads-first FAIL "FAIL $name (allow)" run_in allow-dot

  STUB_TAURI=default,test,wry
  check tauri-with-test FAIL "FAIL $name (tauri)" run_one
  STUB_TAURI=unstable
  check tauri-with-unstable FAIL "FAIL $name (tauri)" run_one
  STUB_TAURI_EXIT=101
  check cargo-tree-of-tauri-fails FAIL "FAIL $name (tree)" run_one
  STUB_LINKED_EXIT=101
  check cargo-tree-of-the-app-fails FAIL "FAIL $name (tree)" run_one

  variant start-other crates/halvo-control/src/onboarding.rs 'pub fn open() {\n    let _ = std::process::Command::new("/usr/bin/open").status();\n}\npub fn ls() {\n    let _ = std::process::Command::new("/bin/ls").output();\n}\n'
  check another-start-in-a-linked-crate FAIL "FAIL $name (start)" run_in start-other
  variant start-twice crates/halvo-control/src/onboarding.rs 'pub fn open() {\n    let _ = std::process::Command::new("/usr/bin/open").status();\n    let _ = std::process::Command::new("/usr/bin/open").status();\n}\n'
  check the-named-limit-twice FAIL "FAIL $name (start)" run_in start-twice
  variant start-app src-tauri/src/agents.rs 'pub fn spawn() {\n    let _ = unsafe { libc::posix_spawn(p, f, a, s, v, e) };\n}\n'
  check a-posix-spawn-in-the-app-crate FAIL "FAIL $name (start)" run_in start-app

  STUB_WRITE=$work/one/crates/halvo-coord/src/words.rs
  check a-test-run-that-changes-a-tracked-file FAIL "FAIL $name (changed)" run_one
  finish
}

finish() {
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
  exit 0
}

case $mode in
  self_test) self_test ;;
  *) measure ;;
esac
