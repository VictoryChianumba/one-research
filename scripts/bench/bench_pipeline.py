#!/usr/bin/env python3
"""Spawn trench under a pty with TRENCH_DEBUG_LOG=1, wait N seconds for the
ingestion pipeline to complete, then send 'q' (clean shutdown) instead of
SIGTERM. Clean shutdown runs trench's cleanup + drop handlers, which is what
flushes buffered env_logger output to disk — SIGTERM kills before that
happens and loses everything except the first second of logs."""
import os, pty, time, signal, sys, select, errno, fcntl, struct, termios

BIN = "/Users/temp/Desktop/projects/pproject-forks/trench/target/release/trench"
WAIT_SECS = float(sys.argv[1]) if len(sys.argv) > 1 else 30.0
ROWS, COLS = 40, 120

env = os.environ.copy()
env["TRENCH_DEBUG_LOG"] = "1"
env["COLUMNS"] = str(COLS)
env["LINES"] = str(ROWS)

pid, fd = pty.fork()
if pid == 0:
    os.execvpe(BIN, [BIN], env)

# Set pty window size so the first draw doesn't panic on a 0x0 area.
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))

# Drain pty output so trench doesn't block on a full pty buffer.
deadline = time.monotonic() + WAIT_SECS
while time.monotonic() < deadline:
    r, _, _ = select.select([fd], [], [], 0.05)
    if r:
        try:
            os.read(fd, 4096)
        except OSError as e:
            if e.errno == errno.EIO:
                break
            raise

# CLEAN SHUTDOWN: send 'q' so trench's main loop hits the quit path,
# runs cleanup, and drop handlers fire (flushing env_logger buffer).
try:
    os.write(fd, b"q")
except OSError:
    pass

# Give trench time to clean up. Drain any remaining output during this window.
cleanup_deadline = time.monotonic() + 2.0
while time.monotonic() < cleanup_deadline:
    r, _, _ = select.select([fd], [], [], 0.05)
    if r:
        try:
            chunk = os.read(fd, 4096)
            if not chunk:
                break
        except OSError as e:
            if e.errno == errno.EIO:
                break
            raise
    try:
        wpid, _ = os.waitpid(pid, os.WNOHANG)
        if wpid != 0:
            break
    except ChildProcessError:
        break

# Fallback: if still alive, force it.
try:
    os.kill(pid, signal.SIGKILL)
    os.waitpid(pid, 0)
except (ProcessLookupError, ChildProcessError):
    pass
print(f"baseline complete; ran for {WAIT_SECS}s before sending 'q'")
