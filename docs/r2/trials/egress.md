cells: macos-15.sandbox macos-15.sandbox.entry macos-15.sandbox.listen windows-2025.wfp windows-2025.wfp.entry windows-2025.wfp.listen this-mac.sandbox this-mac.sandbox.listen
hosted-run branch=scratch/r2-u01-3 commit=3ff65ca296e603c5368a6c17ca0454d0187245d2 run=43017088043 attempt=1
cell macos-15.sandbox runs=1 probes=ok quiet=ok loopback=ok control=ok
cell macos-15.sandbox.entry runs=1 routable=no
cell macos-15.sandbox.listen runs=1 inside=ok
cell windows-2025.wfp runs=1 probes=miss quiet=ok loopback=ok control=ok
cell windows-2025.wfp.entry runs=1 routable=yes
cell windows-2025.wfp.listen runs=1 inside=ok
cell this-mac.sandbox runs=1 probes=ok quiet=ok loopback=ok control=left-out
cell this-mac.sandbox.listen runs=1 inside=ok
not-observable windows-2025 wfp ipv6: the runner has no IPv6 route, so the connect to [2001:db8::1]:443 fails in the network stack before the connect layer of the Filtering Platform, which writes no audit event; the probe exited 1 with the deny rule on and with it off
mechanism sandbox: sandbox-exec runs the command under a profile that allows everything but outbound network operations; it denies every outbound connect and send to an IP address and the connect to the socket /private/var/run/mDNSResponder, through which every name lookup of macOS passes, and allows each loopback entry of the list (127.0.0.1 as ip4 and [::1] as ip6 localhost on the port); a deny rule carries the run's token, and a log stream between two marker denials collects the kernel's reports into denied.log
mechanism sandbox listen: the profile denies no bind, listen or accept, so a process of the tree may listen on a listed loopback entry; a connect from the tree to that entry is allowed by the entry's rule
mechanism sandbox entries: the profile names a host only as localhost (127.0.0.1 or [::1]), so the mode refuses with the cause entry every other host, a routable address and 127.0.0.2 included
mechanism sandbox reports: the kernel's report names the process and its pid, the port, and whether the host is loopback, not the address, so a denied attempt to another host is logged as dest=-:<port>, and the self-test reads an address - as the probe's; the kernel folds repeated reports of one process to one port into one line, which the log expands to one line per attempt with proto - and dest -:<port>
mechanism pf: not built, because the sandbox profile serves on macos-15 (C26.3)
mechanism wfp: the check adds outbound block rules, scoped to every destination outside the list, for the binaries it knows before the start (its runner egress-spawn.exe, the probe egress-probe.exe and the command's executable), under a group named with the run's token and the Windows process id of the check; it turns on the Filtering Platform Connection and Process Creation audits and the DNS client channel, builds the tree from the process-creation events (4688) from the runner down, and logs each blocked connect (5157), each permitted connect to a destination outside the list (5156) and each name lookup (3006) of a process of the tree
mechanism wfp listen: no rule names a bind, a listen or an inbound connection, so a process of the tree may listen on a listed loopback entry, and a connect from the tree to it is outside every block rule and is not logged because the list holds it
mechanism wfp loopback: the firewall does not block a loopback connect, so a connect from the tree to a loopback port outside the list reaches its listener; the permitted-connect audit (5156) logs it, so the run exits 1 with the line
mechanism wfp dns: a name lookup goes through the DNS client service, outside the tree, so the rules do not stop it; the DNS client channel logs it under the pid of the process that asked (two lines for one lookup of the probe, one for each address type)
mechanism wfp unnamed binaries: a binary that the tree starts and that the check did not know before the start has no block rule, so its connect goes out; the permitted-connect audit logs it, so the run exits 1 with the line
mechanism wfp control: with --control-no-deny the check adds no rule and reads the blocked connects alone, so no line of a permitted connect or a lookup is written
verdict egress-modes macos-15: sandbox
verdict egress-modes windows-2025: wfp
verdict egress-modes this-mac: sandbox
