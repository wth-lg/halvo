function fail(n) {
  print "line " n
  bad = 1
  exit 1
}

function ipv4(h,    parts, k, i) {
  if (h !~ /^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$/) return 0
  k = split(h, parts, ".")
  for (i = 1; i <= k; i++) {
    if (length(parts[i]) > 3) return 0
    if (parts[i] ~ /^0./) return 0
    if (parts[i] + 0 > 255) return 0
  }
  return 1
}

function group_ok(g) {
  return g ~ /^[0-9a-f]+$/ && length(g) <= 4 && g !~ /^0./
}

function ipv6(h,    inner, halves, left, right, nl, nr, k, i, groups, n, best, bestlen, run, start, out) {
  if (h !~ /^\[[0-9a-f:]+\]$/) return 0
  inner = substr(h, 2, length(h) - 2)
  k = split(inner, halves, "::")
  if (k > 2) return 0
  n = 0
  if (k == 2) {
    nl = (halves[1] == "") ? 0 : split(halves[1], left, ":")
    nr = (halves[2] == "") ? 0 : split(halves[2], right, ":")
    if (nl + nr > 6) return 0
    for (i = 1; i <= nl; i++) { if (!group_ok(left[i])) return 0; groups[++n] = left[i] }
    for (i = 1; i <= 8 - nl - nr; i++) groups[++n] = "0"
    for (i = 1; i <= nr; i++) { if (!group_ok(right[i])) return 0; groups[++n] = right[i] }
  } else {
    nl = split(inner, left, ":")
    if (nl != 8) return 0
    for (i = 1; i <= 8; i++) { if (!group_ok(left[i])) return 0; groups[++n] = left[i] }
  }
  best = 0; bestlen = 0; run = 0
  for (i = 1; i <= 8; i++) {
    if (groups[i] == "0") {
      if (run == 0) start = i
      run++
      if (run > bestlen) { bestlen = run; best = start }
    } else {
      run = 0
    }
  }
  if (bestlen < 2) best = 0
  out = ""
  for (i = 1; i <= 8; i++) {
    if (best && i == best) { out = out "::"; i = best + bestlen - 1; continue }
    if (out != "" && out !~ /::$/) out = out ":"
    out = out groups[i]
  }
  return out == inner
}

function name(h,    labels, k, i, last) {
  if (length(h) < 1 || length(h) > 253) return 0
  k = split(h, labels, ".")
  for (i = 1; i <= k; i++) {
    if (labels[i] !~ /^[a-z0-9-]+$/ || length(labels[i]) > 63) return 0
    if (labels[i] ~ /^-/ || labels[i] ~ /-$/) return 0
  }
  last = labels[k]
  if (last ~ /^[0-9]+$/) return 0
  if (last ~ /^0x[0-9a-f]*$/) return 0
  return 1
}

{
  total += length($0) + 1
  line = "" $0
  if (NR == 1) {
    if (line != "[fixed]") fail(NR)
    part = "fixed"
    prev = ""
    next
  }
  if (line == "[agent]") {
    if (part != "fixed") fail(NR)
    part = "agent"
    prev = ""
    next
  }
  if (line !~ /^(\[[0-9a-f:]+\]|[a-z0-9.-]+):[1-9][0-9]*$/) fail(NR)
  colon = match(line, /:[1-9][0-9]*$/)
  host = substr(line, 1, colon - 1)
  port = substr(line, colon + 1)
  if (length(port) > 5 || port + 0 > 65535) fail(NR)
  if (host ~ /^\[/) {
    if (!ipv6(host)) fail(NR)
    kind = "v6"
  } else if (host ~ /^[0-9.]+$/) {
    if (!ipv4(host)) fail(NR)
    kind = "v4"
  } else {
    if (!name(host)) fail(NR)
    kind = "name"
  }
  if (prev != "" && !(prev < line)) fail(NR)
  prev = line
  entries[++count] = NR " " part " " kind " " host " " port
}

END {
  if (bad) exit 1
  if (NR == 0) { print "line 1"; exit 1 }
  if (total != size) { print "bytes"; exit 1 }
  if (part != "agent") { print "line " NR + 1; exit 1 }
  for (i = 1; i <= count; i++) print entries[i]
}
