#!/bin/sh
# commit_range_default_ref
# Prints the default branch of origin as this clone last fetched it: the
# branch that refs/remotes/origin/HEAD names, or refs/remotes/origin/main when
# that names a commit, or nothing.
commit_range_default_ref() {
  commit_range_ref=$(git symbolic-ref -q refs/remotes/origin/HEAD 2>/dev/null) || commit_range_ref=
  if [ -z "$commit_range_ref" ] && git rev-parse -q --verify 'refs/remotes/origin/main^{commit}' >/dev/null 2>&1; then
    commit_range_ref=refs/remotes/origin/main
  fi
  [ -z "$commit_range_ref" ] || printf '%s\n' "$commit_range_ref"
}

# commit_range_default_of_remote <remote> <default ref>
# Prints the default branch of <remote> as this clone last fetched it: the
# branch that refs/remotes/<remote>/HEAD names, or refs/remotes/<remote>/main.
# When that names no commit, prints <default ref> if it names a commit, and
# otherwise nothing.
commit_range_default_of_remote() {
  commit_range_ref=$(git symbolic-ref -q "refs/remotes/$1/HEAD" 2>/dev/null) || commit_range_ref=refs/remotes/$1/main
  git rev-parse -q --verify "$commit_range_ref^{commit}" >/dev/null 2>&1 || commit_range_ref=$2
  if [ -n "$commit_range_ref" ] && git rev-parse -q --verify "$commit_range_ref^{commit}" >/dev/null 2>&1; then
    printf '%s\n' "$commit_range_ref"
  fi
}

# commit_range <exempt commit> <default ref> <revision>...
# Prints the commits that git rev-list <revision>... lists, oldest first, one
# full id on each line. When <exempt commit> is not empty and this clone holds
# it, that commit and the commits that it reaches are left out if <default ref>
# holds it, or if <default ref> is empty because the clone knows no default
# branch; a commit that the default branch does not hold is never exempt.
# Returns 2 when git cannot list the commits.
commit_range() {
  commit_range_exempt=$1
  commit_range_default=$2
  shift 2
  if [ -n "$commit_range_exempt" ] && git cat-file -e "$commit_range_exempt^{commit}" 2>/dev/null; then
    if [ -z "$commit_range_default" ] || git merge-base --is-ancestor "$commit_range_exempt" "$commit_range_default" 2>/dev/null; then
      set -- "$@" "^$commit_range_exempt"
    fi
  fi
  git rev-list --reverse --end-of-options "$@" -- </dev/null || return 2
}

# commit_range_read_push_line <line>
# Reads one line that git writes to the standard input of a pre-push hook into
# push_local_ref, push_local_object, push_remote_ref and push_remote_object.
# Returns 1 when push_local_object or push_remote_object is no object id of 40
# or 64 lower-case hexadecimal characters.
commit_range_read_push_line() {
  push_remote_object=${1##* }
  commit_range_rest=${1% *}
  push_remote_ref=${commit_range_rest##* }
  commit_range_rest=${commit_range_rest% *}
  push_local_object=${commit_range_rest##* }
  push_local_ref=${commit_range_rest% *}
  commit_range_is_object_id "$push_local_object" && commit_range_is_object_id "$push_remote_object"
}

# commit_range_push_label
# Prints the name of the push of the line read last.
commit_range_push_label() {
  printf 'push of %s to %s\n' "$push_local_ref" "$push_remote_ref"
}

commit_range_is_object_id() {
  case $1 in
    '' | *[!0123456789abcdef]*) return 1 ;;
  esac
  [ ${#1} = 40 ] || [ ${#1} = 64 ]
}

# commit_range_of_push <exempt commit> <default branch of the remote>
# Prints the commits that the push of the line read last adds, as commit_range
# prints them with <default branch of the remote> as its default ref: the
# commits of the pushed object that are on neither the remote's copy of the
# ref, when this clone holds that commit, nor <default branch of the remote>. Prints nothing for the deletion of a ref and for a tag
# that names no commit. Returns 3 when git cannot resolve the pushed object,
# and 2 when git cannot list the commits.
commit_range_of_push() {
  case $push_local_object in
    *[!0]*) ;;
    *) return 0 ;;
  esac
  if ! git rev-parse -q --verify "$push_local_object^{commit}" >/dev/null 2>&1; then
    git cat-file -e "$push_local_object" 2>/dev/null || return 3
    return 0
  fi
  set -- "$1" "$2" "$push_local_object"
  case $push_remote_object in
    *[!0]*)
      if git cat-file -e "$push_remote_object^{commit}" 2>/dev/null; then set -- "$@" "^$push_remote_object"; fi
      ;;
  esac
  if [ -n "$2" ]; then set -- "$@" "^$2"; fi
  commit_range_exempt=$1
  commit_range_default=$2
  shift 2
  commit_range "$commit_range_exempt" "$commit_range_default" "$@"
}
