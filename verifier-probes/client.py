"""client.py tcp HOST PORT MARK | lookup NAME | sleep SECONDS | timed SECONDS COMMAND...

One call each, with what the system answered on one line.
"""
import socket
import subprocess
import sys
import time

kind = sys.argv[1]
if kind == "tcp":
    host, port, mark = sys.argv[2], int(sys.argv[3]), sys.argv[4]
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.settimeout(5)
    try:
        s.connect((host, port))
        s.sendall((mark + "\n").encode())
        print("client tcp %s:%d mark=%s answer=connected and sent" % (host, port, mark), flush=True)
    except OSError as e:
        print("client tcp %s:%d mark=%s answer=%r" % (host, port, mark, e), flush=True)
    s.close()
elif kind == "lookup":
    try:
        found = sorted(set(i[4][0] for i in socket.getaddrinfo(sys.argv[2], 80)))
        print("client lookup %s answer=%d address(es), first %s" % (sys.argv[2], len(found), found[0]), flush=True)
    except OSError as e:
        print("client lookup %s answer=%r" % (sys.argv[2], e), flush=True)
elif kind == "sleep":
    time.sleep(float(sys.argv[2]))
elif kind == "timed":
    p = subprocess.Popen(sys.argv[3:], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, stdin=subprocess.DEVNULL)
    try:
        out, _ = p.communicate(timeout=float(sys.argv[2]))
        code = p.returncode
    except subprocess.TimeoutExpired:
        p.kill()
        out, _ = p.communicate()
        code = "ended-after-%s-s" % sys.argv[2]
    print("client timed %s exit=%s" % (sys.argv[3], code), flush=True)
    for line in out.decode("utf-8", "replace").splitlines()[:12]:
        print("  | " + line[:260], flush=True)
