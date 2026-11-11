"""mutants.py ID: apply one change, one rule, to the files of the egress check in this checkout.

Each change is an exact replacement that must match once. The caller restores the files with
`git checkout -- scripts/egress` after the run.
"""
import sys

W = "scripts/egress/wfp.ps1"
C = "scripts/egress/check.sh"

M = {
    "W1": ("wfp.ps1: the processes that the root starts are no longer followed", W,
           "if ($tree.ContainsKey($parent)) {",
           "if ($false) {"),
    "W2": ("wfp.ps1: no firewall rule is added (the deny rule is gone, the audit is read as before)", W,
           "if ($Deny -eq 1) { $rules = Add-Rules }",
           "if ($false) { $rules = Add-Rules }"),
    "W3": ("wfp.ps1: a permitted connection is no longer read, a blocked one alone", W,
           "            if ($Deny -ne 1) { continue }\n",
           "            continue\n"),
    "W4": ("wfp.ps1: the events of the DNS client channel are no longer read", W,
           "if ($Deny -eq 1) { $dns = @(Get-Events @{ LogName = $DnsChannel; Id = 3006; StartTime = $since }) }",
           "if ($false) { $dns = @(Get-Events @{ LogName = $DnsChannel; Id = 3006; StartTime = $since }) }"),
    "W5": ("wfp.ps1: a listed address is allowed on every port", W,
           """if ($proto -ne '-' -and $allowed.ContainsKey("$address $port")) { continue }""",
           """if ($proto -ne '-' -and @($allowed.Keys | Where-Object { $_.StartsWith("$address ") }).Count -gt 0) { continue }"""),
    "W6": ("wfp.ps1: the teardown removes no firewall rule", W,
           "    'teardown' {\n        Remove-NetFirewallRule -Group (Get-Group) -ErrorAction SilentlyContinue\n",
           "    'teardown' {\n"),
    "W7": ("wfp.ps1: a udp event is no longer named udp", W,
           "'17' { 'udp' }",
           "'17' { '-' }"),
    "S1": ("check.sh: the profile has no rule for the resolver's socket", C,
           """  printf '(deny network-outbound (remote unix-socket (path-literal "/private/var/run/mDNSResponder")) (with message "%s dns"))\\n' "$1"\n""",
           ""),
    "S2": ("check.sh: the profile denies the resolver's socket alone, no connection", C,
           """  printf '(deny network-outbound (with message "%s other"))\\n' "$1"\n  printf '(allow network-outbound (remote unix-socket))\\n'\n""",
           ""),
}

what, path, old, new = M[sys.argv[1]]
text = open(path, newline="").read()
count = text.count(old)
if count != 1:
    print("mutant %s: the old text matches %d times" % (sys.argv[1], count))
    sys.exit(3)
if sys.argv[1] == "S2":
    loop_old = "  for family in tcp4 tcp6 udp4 udp6; do\n"
    loop_new = "  for family in; do\n"
    if text.count(loop_old) != 1:
        print("mutant S2: the family loop matches %d times" % text.count(loop_old))
        sys.exit(3)
    text = text.replace(loop_old, loop_new)
with open(path, "w", newline="") as f:
    f.write(text.replace(old, new))
print("mutant %s applied: %s" % (sys.argv[1], what))
