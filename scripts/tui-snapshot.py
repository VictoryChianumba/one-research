#!/usr/bin/env python3
"""Render one frame of the trench TUI to plain text at a fixed terminal size.

Much of trench's layout is width-conditional (the narrow/wide split at 100
cols, the responsive right column, the tab-bar nav wrap). This drives the
built binary inside a pseudo-terminal at a chosen size, captures the bytes it
emits, and replays them through a terminal emulator (pyte) to reconstruct the
on-screen cell grid — so a layout change can be eyeballed as text without
firing up the full interactive TUI.

Because it renders the actual binary, it also catches a stale build: run
`cargo build -p trench --release` first, or pass --binary.

Usage:
  scripts/tui-snapshot.py                       # 100x45, Inbox tab
  scripts/tui-snapshot.py --keys '\t'           # press Tab -> Browse
  scripts/tui-snapshot.py --cols 150 --rows 40  # wide layout
  scripts/tui-snapshot.py --binary target/debug/trench

--keys is a literal string with Python escapes interpreted, so '\t' is Tab,
'\x1b' is Esc, etc. Sent after the initial render settles.

Dependency: pip install pyte
"""

import argparse
import os
import pty
import select
import struct
import subprocess
import sys
import termios
import time
import fcntl


def capture(binary, cols, rows, keys, settle, after):
  master, slave = pty.openpty()
  fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", rows, cols, 0, 0))

  env = dict(os.environ)
  env["RUST_LOG"] = "off"  # keep log lines out of the captured screen
  env["TERM"] = "xterm-256color"

  proc = subprocess.Popen(
    [binary],
    stdin=slave, stdout=slave, stderr=subprocess.DEVNULL,
    start_new_session=True, env=env,
  )
  os.close(slave)

  buf = bytearray()

  def drain(seconds):
    end = time.time() + seconds
    while time.time() < end:
      r, _, _ = select.select([master], [], [], 0.2)
      if r:
        try:
          data = os.read(master, 65536)
        except OSError:
          break
        if data:
          buf.extend(data)

  try:
    drain(settle)
    if keys:
      os.write(master, keys.encode())
      drain(after)
  finally:
    proc.terminate()
    try:
      proc.wait(timeout=3)
    except Exception:
      proc.kill()
    os.close(master)

  return bytes(buf)


def main():
  ap = argparse.ArgumentParser(description=__doc__,
                               formatter_class=argparse.RawDescriptionHelpFormatter)
  ap.add_argument("--binary", default="target/release/trench")
  ap.add_argument("--cols", type=int, default=100)
  ap.add_argument("--rows", type=int, default=45)
  ap.add_argument("--keys", default="",
                  help=r"keystrokes to send after first render (e.g. '\t' for Tab)")
  ap.add_argument("--settle", type=float, default=3.5,
                  help="seconds to wait for the initial render + cache load")
  ap.add_argument("--after", type=float, default=2.5,
                  help="seconds to wait after sending --keys")
  args = ap.parse_args()

  try:
    import pyte
  except ImportError:
    sys.exit("error: pyte not installed — run `pip install pyte`")

  if not os.path.exists(args.binary):
    sys.exit(f"error: {args.binary} not found — run `cargo build -p trench --release`")

  keys = args.keys.encode().decode("unicode_escape") if args.keys else ""
  data = capture(args.binary, args.cols, args.rows, keys, args.settle, args.after)

  screen = pyte.Screen(args.cols, args.rows)
  pyte.ByteStream(screen).feed(data)
  for i, line in enumerate(screen.display):
    print(f"{i:2} |{line.rstrip()}")


if __name__ == "__main__":
  main()
