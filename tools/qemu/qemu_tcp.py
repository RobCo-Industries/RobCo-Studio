"""pytest plugin: run RobCo-Core's canary unchanged on Windows.

The suite's harness talks to QEMU over Unix sockets and stops it with
process groups. Python on Windows has neither. This plugin swaps those
few methods for TCP on 127.0.0.1 and plain terminate/kill. Every test
body and every expectation stays the one in RobCo-Core.

Load it with `pytest -p qemu_tcp`, with this folder and RobCo-Core's
`tests` folder on PYTHONPATH.

From RobCo-Core c6b6482 the harness starts QEMU through a /bin/sh
watcher and launcher in a process group, writes a pidfile, and sweeps
stale QEMU processes at session start with ps and os.getuid(). This
plugin replaces start() as before, so neither shell runs and no pidfile
is written. The sweep finds no pidfile, or fails on os.getuid() and
logs one line: it kills nothing on Windows.
"""

import os
import shutil
import signal
import socket
import subprocess
import tempfile
import time

# harness/qemu.py names SIGHUP at import, for its watcher. Windows has
# no SIGHUP. The watcher never runs here, so any value does.
if not hasattr(signal, "SIGHUP"):
    signal.SIGHUP = signal.SIGTERM

from harness import boot as hboot  # noqa: E402
from harness import qemu as hqemu  # noqa: E402

HOST = "127.0.0.1"


def free_port():
    s = socket.socket()
    s.bind((HOST, 0))
    port = s.getsockname()[1]
    s.close()
    return port


def ports(self):
    if not hasattr(self, "_tcp_ports"):
        self._tcp_ports = {}
    return self._tcp_ports


def port_for(self, name):
    p = ports(self)
    if name not in p:
        p[name] = free_port()
    return p[name]


def command_line(self):
    cmd = [self.binary, "-M", self.machine, "-display", "none",
           "-monitor", "none", "-no-user-config"]
    if self.serial_count:
        for path in self.serial_paths:
            cmd += ["-serial", f"file:{path}"]
    elif self.null_serial:
        cmd += ["-serial", "null"]
    if self.use_qmp:
        cmd += ["-qmp", f"tcp:{HOST}:{port_for(self, 'qmp')},server=on,wait=off"]
    if self.use_qtest:
        cmd += ["-qtest", f"tcp:{HOST}:{port_for(self, 'qtest')}"]
        if self.qtest_accel:
            cmd += ["-accel", self.qtest_accel]
    # boot() adds serial chardevs on Unix socket paths: move them to TCP.
    extra = []
    for arg in self.extra:
        if arg.startswith("socket,id=ser") and ",path=" in arg:
            head, rest = arg.split(",path=", 1)
            path, tail = rest.split(",", 1)
            arg = f"{head},host={HOST},port={port_for(self, path)},{tail}"
        extra.append(arg)
    return cmd + extra


def connect(port, proc, stderr, timeout):
    deadline = time.monotonic() + timeout
    while True:
        if proc.poll() is not None:
            raise hqemu.QemuError(f"QEMU exited ({proc.returncode}): {stderr()}")
        try:
            return socket.create_connection((HOST, port), timeout=1)
        except OSError:
            if time.monotonic() > deadline:
                raise hqemu.QemuError(f"connect timeout: port {port}")
            time.sleep(0.02)


def start(self):
    qt_listener = None
    if self.use_qtest:
        qt_listener = socket.socket()
        qt_listener.bind((HOST, port_for(self, "qtest")))
        qt_listener.listen(1)
        qt_listener.settimeout(self.timeout)
    err = open(self.stderr_path, "wb")
    self.proc = subprocess.Popen(self.command_line(), stdin=subprocess.DEVNULL,
                                 stdout=err, stderr=err)
    err.close()
    try:
        if qt_listener:
            try:
                self._qt, _ = qt_listener.accept()
            except socket.timeout:
                raise hqemu.QemuError("qtest connect timeout: " + self.stderr())
            self._qt.settimeout(self.timeout)
            self._qt_file = self._qt.makefile("rwb", buffering=0)
        if self.use_qmp:
            self._connect_qmp()
    except Exception:
        self.close()
        raise
    finally:
        if qt_listener:
            qt_listener.close()
    return self


def connect_qmp(self):
    s = connect(port_for(self, "qmp"), self.proc, self.stderr, self.timeout)
    s.settimeout(self.timeout)
    self._qmp, self._qmp_buf = s, b""
    self._qmp_read()
    self.execute("qmp_capabilities")


def stop_proc(self):
    if self.proc.poll() is None:
        self.proc.terminate()
    try:
        self.proc.wait(3)
    except subprocess.TimeoutExpired:
        self.proc.kill()
        self.proc.wait()


def connect_serial(path, qemu, timeout):
    # The harness's reader thread treats a recv timeout as a closed
    # connection, so the serial socket must block, as the Unix one does.
    s = connect(port_for(qemu, path), qemu.proc, qemu.stderr, timeout)
    s.settimeout(None)
    return s


def build_fwstub():
    """The stub's Makefile rule without make: gcc and objcopy only."""
    src = os.path.join(hboot.FWSTUB_DIR, "fwstub.S")
    out = tempfile.mkdtemp(prefix="fwstub")
    elf, binary = os.path.join(out, "fwstub.elf"), os.path.join(out, "fwstub.bin")
    subprocess.run(["arm-none-eabi-gcc", "-mcpu=arm1176jzf-s", "-marm", "-nostdlib",
                    "-Wl,-Ttext=0x0", "-Wl,--build-id=none", "-Wl,--no-warn-rwx-segments",
                    "-o", elf, src], check=True)
    subprocess.run(["arm-none-eabi-objcopy", "-O", "binary", elf, binary], check=True)
    return binary


hqemu.Qemu.command_line = command_line
hqemu.Qemu.start = start
hqemu.Qemu._connect_qmp = connect_qmp
hqemu.Qemu._stop_proc = stop_proc
hboot._connect_unix = connect_serial
if os.name == "nt" and shutil.which("arm-none-eabi-gcc"):
    hboot._stub_path = build_fwstub()
