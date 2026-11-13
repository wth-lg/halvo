#!/bin/sh
# The place of a built development body of halvo-sample, and the step that makes sure it is there. Checks
# source this file; it holds functions alone.

r2_target_dir() {
  case ${CARGO_TARGET_DIR:-} in
    '') printf '%s/target\n' "$1" ;;
    /* | [A-Za-z]:*) printf '%s\n' "$CARGO_TARGET_DIR" ;;
    *) printf '%s/%s\n' "$1" "$CARGO_TARGET_DIR" ;;
  esac
}

r2_body_dir() {
  r2_root=$(r2_target_dir "$1") || return 1
  printf '%s/r2/%s\n' "$r2_root" "$2"
}

r2_body_bin() {
  r2_root=$(r2_target_dir "$1") || return 1
  r2_bin=$r2_root/debug/halvo-sample
  [ -f "$r2_bin.exe" ] && r2_bin=$r2_bin.exe
  printf '%s\n' "$r2_bin"
}

r2_body_check() {
  perl -MDigest::SHA -MFile::Find -e '
    my ($dir, $committed) = @ARGV;
    open(my $m, "<:raw", $committed) or exit 2;
    my $want_text = do { local $/; <$m> };
    close $m;
    my %want;
    while ($want_text =~ /^\[\[file\]\]\npath = "([^"\n]*)"\nsha256 = "([0-9a-f]{64})"$/mg) { $want{$1} = $2 }
    my %have;
    if (-d $dir) {
      find({ no_chdir => 1, wanted => sub {
        return unless -f $_;
        my $rel = substr($_, length($dir) + 1);
        $rel =~ s{\\}{/}g;
        $have{$rel} = 1;
      } }, $dir);
    }
    my (@differ, @missing, @extra);
    if (!$have{"manifest.toml"}) {
      push @missing, "manifest.toml";
    } else {
      open(my $h, "<:raw", "$dir/manifest.toml") or exit 2;
      my $have_text = do { local $/; <$h> };
      close $h;
      push @differ, "manifest.toml" if $have_text ne $want_text;
    }
    for my $p (sort keys %want) {
      if (!$have{$p}) { push @missing, $p; next }
      my $sha = Digest::SHA->new(256);
      $sha->addfile("$dir/$p", "b");
      push @differ, $p if $sha->hexdigest ne $want{$p};
    }
    for my $p (sort keys %have) {
      push @extra, $p unless $p eq "manifest.toml" || exists $want{$p};
    }
    my @all = sort (@differ, @missing, @extra);
    if (!@all) { print "same\n"; exit 0 }
    printf "differ %d %d %d %s\n", scalar(@differ), scalar(@missing), scalar(@extra), $all[0];
  ' "$1" "$2"
}

r2_body_line() {
  r2_rest=${1#differ }
  r2_d=${r2_rest%% *}
  r2_rest=${r2_rest#* }
  r2_m=${r2_rest%% *}
  r2_rest=${r2_rest#* }
  r2_x=${r2_rest%% *}
  r2_first=${r2_rest#"$r2_x"}
  r2_first=${r2_first# }
  r2_line="files that differ: $r2_d, missing: $r2_m, extra: $r2_x"
  if printf '%s\n' "$r2_first" | grep -Eqx 'streams/[a-z0-9-]{1,48}/(day-[0-9]{3}|labels)\.jsonl|manifest\.toml|script-manifest\.jsonl|layers/steps\.jsonl'; then
    r2_line="$r2_line, the first $r2_first"
  fi
  printf '%s\n' "$r2_line"
}

r2_body_rename() {
  perl -e 'rename($ARGV[0], $ARGV[1]) or exit 1' "$1" "$2"
}

# Moves the new body $1 to its place $2. A body in place that does not check same against the committed
# manifest $3 is moved aside into the call's own reserved directory $4, under a name that no other try uses.
r2_body_place() {
  r2_tries=0
  while [ "$r2_tries" -lt 3 ]; do
    r2_body_rename "$1" "$2" && return 0
    r2_state=$(r2_body_check "$2" "$3") || r2_state=unread
    [ "$r2_state" = same ] && return 0
    r2_body_rename "$2" "$4/aside.$r2_tries" || true
    r2_tries=$((r2_tries + 1))
  done
  return 1
}

# Builds the body into a directory that mktemp reserves for this call alone, places it, and on every way out
# removes that directory with what stands in it: a body that was not placed and a body that was moved aside.
r2_body_build() {
  r2_dir=$(r2_body_dir "$1" "$2") || return 1
  mkdir -p "${r2_dir%/*}" || return 1
  r2_slot=$(mktemp -d "$r2_dir.new.XXXXXX") || return 1
  r2_body_make "$@"
  r2_made=$?
  rm -rf -- "$r2_slot" 2>/dev/null || printf 'body %s: its reserved directory cannot be removed\n' "$2"
  return "$r2_made"
}

r2_body_make() {
  r2_new=$r2_slot/body
  r2_seed=$(sed -n 's/^seed = //p' "$5") || return 1
  r2_started=$(date +%s)
  (cd "$4" && "$3" build --tier "$2" --seed "$r2_seed" --out "$r2_new" ./*.jsonl) >"$6" 2>&1 || {
    printf 'body %s: build failed\n' "$2"
    return 1
  }
  r2_state=$(r2_body_check "$r2_new" "$5") || r2_state=unread
  if [ "$r2_state" != same ]; then
    if [ "$r2_state" = unread ]; then
      printf 'body %s: the built body cannot be read\n' "$2"
    else
      r2_line=$(r2_body_line "$r2_state") || r2_line=unread
      printf 'body %s: %s\n' "$2" "$r2_line"
    fi
    return 1
  fi
  r2_body_place "$r2_new" "$r2_dir" "$5" "$r2_slot" || {
    printf 'body %s: the built body cannot be moved into place\n' "$2"
    return 1
  }
  r2_ended=$(date +%s) || r2_ended=$r2_started
  printf 'body %s built %s s\n' "$2" "$((r2_ended - r2_started))"
}

r2_body_ensure() {
  r2_dir=$(r2_body_dir "$1" "$2") || return 1
  if [ -f "$r2_dir/manifest.toml" ] && cmp -s "$r2_dir/manifest.toml" "$5"; then
    r2_state=$(r2_body_check "$r2_dir" "$5") || r2_state=unread
    if [ "$r2_state" = same ]; then
      printf 'body %s reused\n' "$2"
      return 0
    fi
  fi
  r2_body_build "$@"
}
