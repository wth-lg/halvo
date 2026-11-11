"""sink.py PORT FILE: a listener on 127.0.0.1:PORT, outside the wrapped tree, that records what arrives.

One line per accepted connection in FILE: the number of bytes and the first 60 of them.
"""
import socket
import sys

port = int(sys.argv[1])
out = sys.argv[2]


def note(text):
    with open(out, "a") as f:
        f.write(text + "\n")


s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", port))
s.listen(16)
note("sink port=%d ready" % port)
while True:
    conn, _ = s.accept()
    conn.settimeout(3)
    data = b""
    try:
        while len(data) < 400:
            chunk = conn.recv(400)
            if not chunk:
                break
            data += chunk
            if b"\n" in data:
                break
    except (socket.timeout, OSError):
        pass
    if data.startswith(b"GET "):
        try:
            conn.sendall(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
        except OSError:
            pass
    note("sink port=%d accepted bytes=%d first=%r" % (port, len(data), data[:60]))
    conn.close()
