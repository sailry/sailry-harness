import json
import os
import stat
from pathlib import Path
import socket
import subprocess
import sys

home = Path.home()
path = home / ".sailry-host/control.sock"
metadata = path.lstat()
if not stat.S_ISSOCK(metadata.st_mode) or metadata.st_uid != os.geteuid() or metadata.st_mode & 0o077:
    raise RuntimeError("Host control socket must be private and owned by the current user")
with socket.socket(socket.AF_UNIX) as connection:
    connection.settimeout(5)
    connection.connect(str(path))
    connection.sendall(b'{"command":"node_hello","version":1}\n')
    with connection.makefile("rb") as stream:
        line = stream.readline(65537)
    if len(line) > 65536:
        raise RuntimeError("Host identity response exceeds the limit")
response = json.loads(line)
if response.get("version") != 1 or response.get("body", {}).get("kind") != "hello":
    raise RuntimeError("Host identity response is invalid")
binary = Path(sys.argv[1]) / "sailry-host"
version = subprocess.check_output([str(binary), "--version"], text=True, timeout=5).strip()
print(json.dumps({"node": response["node"], "version": version}))
