function jstr(line, key,    at, i, c, out, n) {
  at = index(line, "\"" key "\":\"")
  if (at == 0) return ""
  i = at + length(key) + 4
  out = ""
  n = length(line)
  while (i <= n) {
    c = substr(line, i, 1)
    if (c == "\"") return out
    if (c == "\\") {
      i++
      c = substr(line, i, 1)
      if (c == "n") out = out "\n"
      else if (c == "t") out = out " "
      else if (c == "u") { out = out "?"; i += 4 }
      else out = out c
    } else {
      out = out c
    }
    i++
  }
  return ""
}

function days(y, m, d,    era, yoe, doy, doe) {
  if (m <= 2) y--
  era = int(y / 400)
  yoe = y - era * 400
  doy = int((153 * (m > 2 ? m - 3 : m + 9) + 2) / 5) + d - 1
  doe = yoe * 365 + int(yoe / 4) - int(yoe / 100) + doy
  return era * 146097 + doe - 719468
}

function instant(ts,    secs, frac, tz, sign, offset) {
  if (ts !~ /^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9] [0-9][0-9]:[0-9][0-9]:[0-9][0-9]\.[0-9]+[-+][0-9][0-9][0-9][0-9]$/) return "-"
  secs = days(substr(ts, 1, 4) + 0, substr(ts, 6, 2) + 0, substr(ts, 9, 2) + 0) * 86400
  secs += substr(ts, 12, 2) * 3600 + substr(ts, 15, 2) * 60 + substr(ts, 18, 2)
  frac = substr(ts, 21)
  tz = substr(frac, length(frac) - 4)
  frac = substr(frac, 1, length(frac) - 5)
  sign = substr(tz, 1, 1) == "-" ? -1 : 1
  offset = substr(tz, 2, 2) * 3600 + substr(tz, 4, 2) * 60
  secs -= sign * offset
  while (length(frac) < 9) frac = frac "0"
  return sprintf("%d%s", secs, substr(frac, 1, 9))
}

function emit(at, proto, dest, pid, image) {
  printf "denied at=%s proto=%s dest=%s pid=%s image=%s\n", at, proto, dest, pid, image
}

{
  if (jstr($0, "processImagePath") != "/kernel") next
  if (jstr($0, "senderImagePath") !~ /\/Sandbox$/) next
  msg = jstr($0, "eventMessage")
  cut = index(msg, "\n")
  if (cut == 0) next
  head = substr(msg, 1, cut - 1)
  tag = substr(msg, cut + 1)
  if (index(tag, token " ") != 1) next
  tag = substr(tag, length(token) + 2)
  if (tag == "mark-start") { open = 1; next }
  if (tag == "mark-end") { open = 0; next }
  if (!open) next
  dup = 0
  if (head ~ /^[0-9]+ duplicate reports? for /) {
    dup = substr(head, 1, index(head, " ") - 1) + 0
    head = substr(head, index(head, " for ") + 5)
  }
  if (index(head, "Sandbox: ") != 1) next
  head = substr(head, 10)
  deny = index(head, ") deny(")
  if (deny == 0) next
  who = substr(head, 1, deny)
  target = substr(head, deny + 1)
  sub(/^\) deny\([0-9]*\) network-outbound /, "", target)
  open_paren = 0
  for (i = length(who); i > 0; i--) if (substr(who, i, 1) == "(") { open_paren = i; break }
  pid = substr(who, open_paren + 1, length(who) - open_paren - 1)
  image = substr(who, 1, open_paren - 1)
  gsub(/[ \t]/, "_", image)
  if (pid !~ /^[0-9]+$/) pid = "-"
  if (image == "") image = "-"
  port = target
  sub(/^.*:/, "", port)
  if (port !~ /^[0-9]+$/) port = "-"
  at = instant(jstr($0, "timestamp"))
  split(tag, t, " ")
  if (tag == "dns") { proto = "dns"; dest = "resolver" }
  else if (t[1] ~ /^(tcp|udp)[46]$/) {
    proto = substr(t[1], 1, 3)
    if (t[2] == "lo" && substr(t[1], 4, 1) == "4") dest = "127.0.0.1:" port
    else dest = "-:" port
  } else if (t[1] ~ /^icmp[46]$/) { proto = "icmp"; dest = "-:-" }
  else { proto = "-"; dest = "-" }
  if (dup == 0) { emit(at, proto, dest, pid, image); next }
  if (target != "/private/var/run/mDNSResponder") { proto = "-"; dest = "-:" port }
  for (k = 0; k < dup; k++) emit(at, proto, dest, pid, image)
}
