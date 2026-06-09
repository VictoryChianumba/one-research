#!/usr/bin/env python3
"""Spawn one-research --bench-startup under a pty, wait for natural exit, and
print:
 - first_frame_ready_ms (from one-research's stdout, source-of-truth instrumented)
 - wall_clock_ms (Python's measure of fork→exit, includes process startup +
   binary load + dyld + std init + first frame + clean exit)

Run N times for a distribution."""
import os, pty, time, signal, sys, select, errno, fcntl, struct, termios

BIN = "/Users/temp/Desktop/projects/pproject-forks/one-research/target/release/one-research"
N = int(sys.argv[1]) if len(sys.argv) > 1 else 5
TIMEOUT_SECS = 5.0
ROWS, COLS = 40, 120

env = os.environ.copy()
env["COLUMNS"] = str(COLS)
env["LINES"] = str(ROWS)

def _set_pty_size(fd, rows, cols):
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

def one_run():
    start = time.monotonic()
    pid, fd = pty.fork()
    if pid == 0:
        os.execvpe(BIN, [BIN, "--bench-startup"], env)
    # Set the pty window size BEFORE the child queries it via TIOCGWINSZ.
    # Without this the slave reports 0x0 and ratatui's first draw panics.
    _set_pty_size(fd, ROWS, COLS)
    out = b""
    deadline = time.monotonic() + TIMEOUT_SECS
    while time.monotonic() < deadline:
        r, _, _ = select.select([fd], [], [], 0.02)
        if r:
            try:
                chunk = os.read(fd, 4096)
                if not chunk:
                    break
                out += chunk
            except OSError as e:
                if e.errno == errno.EIO:
                    break
                raise
        try:
            wpid, status = os.waitpid(pid, os.WNOHANG)
            if wpid != 0:
                break
        except ChildProcessError:
            break
    wall = (time.monotonic() - start) * 1000.0
    # Cleanup if still alive
    try:
        os.kill(pid, signal.SIGKILL)
        os.waitpid(pid, 0)
    except (ProcessLookupError, ChildProcessError):
        pass
    text = out.decode("utf-8", errors="replace")
    ffr = None
    for line in text.splitlines():
        if "first_frame_ready_ms=" in line:
            try:
                ffr = int(line.strip().split("=", 1)[1])
            except ValueError:
                pass
            break
    return ffr, wall

ffrs, walls = [], []
for i in range(N):
    ffr, wall = one_run()
    ffrs.append(ffr)
    walls.append(wall)
    print(f"run {i+1}: first_frame_ready={ffr}ms wall={wall:.1f}ms")

valid_ffrs = [x for x in ffrs if x is not None]
if valid_ffrs:
    print(f"\nfirst_frame_ready_ms: min={min(valid_ffrs)} max={max(valid_ffrs)} "
          f"mean={sum(valid_ffrs)/len(valid_ffrs):.1f} "
          f"median={sorted(valid_ffrs)[len(valid_ffrs)//2]}")
print(f"wall_clock_ms:        min={min(walls):.1f} max={max(walls):.1f} "
      f"mean={sum(walls)/len(walls):.1f} "
      f"median={sorted(walls)[len(walls)//2]:.1f}")
