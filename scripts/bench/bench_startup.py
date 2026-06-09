#!/usr/bin/env python3
"""Spawn one-research under a pty with ONE_RESEARCH_DEBUG_LOG=1, kill after T seconds,
then dump the new one-research.log. Used to baseline startup timing without a
human at the keyboard."""
import os, pty, time, signal, sys, select, errno

BIN = "/Users/temp/Desktop/projects/pproject-forks/one-research/target/release/one-research"
WAIT_SECS = float(sys.argv[1]) if len(sys.argv) > 1 else 1.5

env = os.environ.copy()
env["ONE_RESEARCH_DEBUG_LOG"] = "1"
# Force a known terminal size so the first draw isn't doing tty-size
# negotiation against whatever shape my pty defaulted to.
env["COLUMNS"] = "120"
env["LINES"] = "40"

pid, fd = pty.fork()
if pid == 0:
    os.execvpe(BIN, [BIN], env)
else:
    # Drain pty output so one-research doesn't block on a full pty buffer.
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
    # Polite shutdown — gives the panic hook a chance to restore the tty,
    # but we don't care about the tty (it's a throwaway pty).
    try:
        os.kill(pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    # Give it a moment to flush its log.
    time.sleep(0.2)
    try:
        os.kill(pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    os.waitpid(pid, 0)
    print(f"baseline complete; killed after {WAIT_SECS}s wall-clock")
