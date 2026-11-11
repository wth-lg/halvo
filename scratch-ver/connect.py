import errno
import socket
import sys
import time

host, port = sys.argv[1], int(sys.argv[2])
family = socket.AF_INET6 if ":" in host else socket.AF_INET
s = socket.socket(family, socket.SOCK_STREAM)
s.settimeout(5)
t = time.time()
try:
    s.connect((host, port))
    result = "connected"
except socket.timeout:
    result = "timeout (no answer in 5 s: the SYN was sent)"
except OSError as e:
    result = "errno=%s %s" % (e.errno, errno.errorcode.get(e.errno, "?"))
line = "connect %s:%d %s after %.1fs" % (host, port, result, time.time() - t)
if len(sys.argv) > 3:
    with open(sys.argv[3], "a") as f:
        f.write(line + "\n")
print(line)
