#!/bin/sh
# jobs: windows
# rule: No allow of clippy's disallowed_methods stands outside test code of halvo-coord, halvo-coord-app and halvo-coord-store, each of which keeps its clippy.toml with Command::new, spawn, output and status, a normal build of the workspace resolves tauri with neither unstable nor test, the crates that the app links start no child but the two named limits, every test of halvo-coord, and of halvo-coord-app once that crate is in the tree, passes with the committed lockfile and changes no tracked file, and every refusal test of I3.6, I4.8 and I5.11 that the crates hold ran and passed.
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
  my @crates = @rest;
  my ($rust, $tests, $product) = (0, 0, 0);
  my %has = map { $_ => 1 } keys %blob;
  my @methods = map { "std::process::Command::$_" } qw(new spawn output status);
  for my $cr (@crates) {
    next unless $has{"$cr/Cargo.toml"};
    my $toml = $blob{"$cr/clippy.toml"};
    my @missing = defined $toml ? grep { $toml !~ /path\s*=\s*"\Q$_\E"/ } @methods : ('clippy.toml');
    for my $m (@missing) { print $out "allow $cr/clippy.toml lacks $m\n"; $found++ }
    if ($has{"$cr/.clippy.toml"}) { print $out "allow $cr/.clippy.toml stands\n"; $found++ }
    my @rs = sort grep { m{\A\Q$cr\E/.+\.rs\z} } keys %blob;
    $rust += @rs;
    my @intests = grep { m{\A\Q$cr\E/tests/} } @rs;
    $tests += @intests;
    my %declared;
    for my $f (grep { !m{\A\Q$cr\E/tests/} } @rs) {
      for my $ch (children($f)) { $declared{$ch->[0]} = 1 if defined $ch->[0] }
    }
    my @roots = grep { !m{\A\Q$cr\E/tests/} && !$declared{$_} } @rs;
    my ($kind) = walk(@roots);
    for my $f (grep { !m{\A\Q$cr\E/tests/} } @rs) {
      if (($kind->{$f} // 'product') eq 'test') { $tests++; next }
      $product++;
      my $re = qr/\bdisallowed_methods?\b|\b(?:allow|expect)\s*\([^()]*?(?:\bclippy\s*::\s*(?:all|style)\b|\bwarnings\b)|#\s*!?\s*\[\s*path\s*=|\binclude\s*!/;
      for my $h (hits($f, $re)) { print $out "allow $f:$h->[0]\n"; $found++ }
    }
  }
  my @tomls = grep { $has{$_} } ('Cargo.toml', '.cargo/config.toml', '.cargo/config', map { "$_/Cargo.toml" } @crates);
  for my $t (@tomls) {
    my $cfg = $t =~ m{\A\.cargo/};
    my $i = 0;
    for my $line (split /\n/, $blob{$t}) {
      $i++;
      next if $line =~ /\A\s*#/;
      my $bad = $line =~ /\bdisallowed_methods?\b|\bclippy\s*::\s*(?:all|style)\b|cap-lints/
        || $line =~ /\b(?:all|style|warnings)\b\s*=\s*(?:\{[^}]*\blevel\s*=\s*)?"(?:allow|expect)"/
        || ($cfg && $line =~ /warnings|"\s*-A|--allow/);
      if ($bad) { print $out "allow $t:$i\n"; $found++ }
    }
  }
  printf "allow crates=%d rust_files=%d test_files=%d product_files=%d manifests=%d found=%d\n",
    scalar(grep { $has{"$_/Cargo.toml"} } @crates), $rust, $tests, $product, scalar @tomls, $found;
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

blobs() {
  label=$1
  shift
  git ls-files -s -z -- "$@" >"$artifacts/$label.list" || fail setup
  perl "$scan" ids "$artifacts/$label.list" >"$artifacts/$label.ids" || fail setup
  git cat-file --batch <"$artifacts/$label.ids" >"$artifacts/$label.blobs" || fail setup
}

lint_allows() {
  blobs allow crates/halvo-coord crates/halvo-coord-app crates/halvo-coord-store Cargo.toml .cargo
  perl "$scan" allow "$artifacts/allow.list" "$artifacts/allow.blobs" "$artifacts/allow.txt" \
    crates/halvo-coord crates/halvo-coord-app crates/halvo-coord-store
  code=$?
  case $code in
    0) ;;
    1) fail allow ;;
    *) fail setup ;;
  esac
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
  lint_allows
  tauri_features
  child_starts
  git diff --binary >"$artifacts/tracked-before.txt" || fail setup
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
    export PATH STUB_OUT STUB_EXIT STUB_ARGS STUB_TAURI STUB_TAURI_EXIT STUB_LINKED_EXIT STUB_LINKED STUB_WRITE
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
  STUB_TAURI_EXIT=0
  STUB_LINKED_EXIT=0
  STUB_TAURI=default,wry
  STUB_WRITE=
}

put() {
  mkdir -p "${1%/*}" || exit 2
  printf '%b' "$2" >"$1" || exit 2
}

lint_toml() {
  for method in new spawn output status; do
    printf '{ path = "std::process::Command::%s", reason = "fixture" },\n' "$method"
  done
}

crate() {
  put "$1/Cargo.toml" "[package]\nname = \"${1##*/}\"\nversion = \"0.0.0\"\n"
  {
    printf 'disallowed-methods = [\n'
    lint_toml
    printf ']\n'
  } >"$1/clippy.toml" || exit 2
}

fixture() {
  dir=$work/$1
  mkdir -p "$dir/crates/halvo-coord/tests" || exit 2
  printf '#[test]\nfn refusal_two_models() {}\n#[test]\nfn refusal_queue_full() {}\n#[test]\nfn refusal_not_a_case() {}\n' >"$dir/crates/halvo-coord/tests/refusal.rs" || exit 2
  crate "$dir/crates/halvo-coord"
  put "$dir/crates/halvo-coord/src/lib.rs" '//! No #[allow(clippy::disallowed_methods)] here.\npub mod words;\n#[cfg(test)]\nmod tests;\n#[cfg(all(test, unix))]\nmod more {\n    const B: char = '"'"'}'"'"';\n    const S: &str = "}{";\n    #[allow(clippy::disallowed_methods)]\n    fn f() {}\n}\npub fn after() -> &'"'"'static str {\n    r#"x"y #[allow(clippy::disallowed_methods)] }"#\n}\n'
  put "$dir/crates/halvo-coord/src/words.rs" 'pub fn w() {}\n'
  put "$dir/crates/halvo-coord/src/tests.rs" 'mod deep;\n#[allow(clippy::disallowed_methods)]\nfn t() {}\n'
  put "$dir/crates/halvo-coord/src/tests/deep.rs" '#[expect(clippy::disallowed_methods)]\nfn d() {}\n'
  put "$dir/crates/halvo-coord/tests/words_same.rs" '#[expect(clippy::disallowed_methods, reason = "fixture")]\nfn s() {}\n'
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
case $* in
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
  run_one() {
    CI_D_REPO=$work/one CI_D_ARTIFACTS=$work/artifacts-one CI_D_JOB=self-test sh "$here/$name.sh"
  }
  run_in() {
    CI_D_REPO=$work/$1 CI_D_ARTIFACTS=$work/artifacts-$1 CI_D_JOB=self-test sh "$here/$name.sh"
  }

  output pass "$ok_both" "$ok_queue" "$ok_other" "$done_line"
  check pass PASS 'refusals=2' run_one
  check app-crate-in-the-tree PASS 'halvo-coord-app=yes' \
    env CI_D_REPO="$work/app" CI_D_ARTIFACTS="$work/artifacts-app" sh "$here/$name.sh"
  check allows-in-test-code-only PASS 'allow crates=1 rust_files=6 test_files=4 product_files=2 manifests=1 found=0' run_one
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

  variant allow-product crates/halvo-coord-app/src/lib.rs '#[allow(clippy::disallowed_methods)]\npub fn a() {}\n'
  check an-allow-in-library-code FAIL "FAIL $name (allow)" run_in allow-product
  variant allow-expect crates/halvo-coord/src/words.rs 'pub fn w() {\n    #[expect(clippy::disallowed_method)]\n    let _ = 1;\n}\n'
  check an-expect-of-the-old-name-in-a-module FAIL "FAIL $name (allow)" run_in allow-expect
  variant allow-group crates/halvo-coord-app/src/lib.rs '#![allow(clippy::style)]\npub fn a() {}\n'
  check an-allow-of-the-lint-group FAIL "FAIL $name (allow)" run_in allow-group
  variant allow-after-test crates/halvo-coord/src/words.rs '#[cfg(test)]\nmod t {\n    const S: &str = "{";\n}\n#[allow(clippy::disallowed_methods)]\npub fn w() {}\n'
  check an-allow-after-a-test-module-with-a-brace-in-a-string FAIL "FAIL $name (allow)" run_in allow-after-test
  variant allow-after-use crates/halvo-coord/src/words.rs '#[cfg(test)]\nuse std::fmt;\npub fn w() {\n    #[allow(clippy::disallowed_methods)]\n    let _ = 1;\n}\n'
  check an-allow-after-a-test-item-that-ends-with-a-semicolon FAIL "FAIL $name (allow)" run_in allow-after-use
  variant allow-any-test crates/halvo-coord/src/words.rs '#[cfg(any(test, feature = "x"))]\n#[allow(clippy::disallowed_methods)]\npub fn w() {}\n'
  check an-allow-under-cfg-any-test FAIL "FAIL $name (allow)" run_in allow-any-test
  variant allow-orphan crates/halvo-coord/src/tests/orphan.rs '#[allow(clippy::disallowed_methods)]\nfn o() {}\n'
  check an-allow-in-a-file-that-no-test-module-declares FAIL "FAIL $name (allow)" run_in allow-orphan
  variant allow-manifest crates/halvo-coord/Cargo.toml '[package]\nname = "halvo-coord"\n\n[lints.clippy]\ndisallowed_methods = "allow"\n'
  check an-allow-in-the-manifest FAIL "FAIL $name (allow)" run_in allow-manifest
  variant allow-config .cargo/config.toml '[build]\nrustflags = ["-Aclippy::disallowed_methods"]\n'
  check an-allow-in-the-cargo-configuration FAIL "FAIL $name (allow)" run_in allow-config
  variant allow-short crates/halvo-coord-app/clippy.toml 'disallowed-methods = [\n{ path = "std::process::Command::new" },\n]\n'
  check a-clippy-toml-without-spawn FAIL "FAIL $name (allow)" run_in allow-short
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
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $mode in
  self_test) self_test ;;
  *) measure ;;
esac
