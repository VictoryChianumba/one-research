"""Drive the One-Research TUI in a real pseudo-terminal and record screens.

usage: run_app.py BIN HOME OUT_DIR SECONDS [STEP ...]
  STEP = "<t>:keys:<text>"  send text at t seconds (\\x14 = Ctrl+T, \\r = Enter,
                            \\x1b = Esc; written with python escapes)
       | "<t>:shot:<name>"  save the current screen as OUT_DIR/<name>.txt
A final screen is always saved as OUT_DIR/final.txt. The app is SIGTERMed at
SECONDS; exit info goes to OUT_DIR/exit.txt.
"""
import codecs, os, pty, select, signal, struct, sys, time, fcntl, termios

import pyte

ROWS, COLS = 40, 140

bin_path, home, out_dir, seconds = sys.argv[1], sys.argv[2], sys.argv[3], float(sys.argv[4])
steps = []
for s in sys.argv[5:]:
    t, kind, arg = s.split(":", 2)
    steps.append((float(t), kind, codecs.decode(arg, "unicode_escape")))
steps.sort(key=lambda x: x[0])
os.makedirs(out_dir, exist_ok=True)

screen = pyte.Screen(COLS, ROWS)
stream = pyte.ByteStream(screen)

pid, fd = pty.fork()
if pid == 0:
    env = {k: v for k, v in os.environ.items() if not k.startswith("XDG_")}
    env.update(HOME=home, TERM="xterm-256color")
    os.execve(bin_path, [bin_path], env)

fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", ROWS, COLS, 0, 0))
os.kill(pid, signal.SIGWINCH)


def save(name):
    with open(os.path.join(out_dir, name + ".txt"), "w") as f:
        f.write("\n".join(line.rstrip() for line in screen.display).rstrip() + "\n")


start = time.time()
exited = None
while True:
    now = time.time() - start
    while steps and steps[0][0] <= now:
        _, kind, arg = steps.pop(0)
        if kind == "keys":
            for ch in arg:  # one key per write, like a typist
                os.write(fd, ch.encode())
                time.sleep(0.15)
        elif kind == "shot":
            save(arg)
    if now >= seconds:
        break
    r, _, _ = select.select([fd], [], [], 0.1)
    if r:
        try:
            data = os.read(fd, 65536)
        except OSError:
            data = b""
        if not data:
            break
        stream.feed(data)
    done, status = os.waitpid(pid, os.WNOHANG)
    if done:
        exited = status
        break

save("final")
if exited is None:
    os.kill(pid, signal.SIGTERM)
    for _ in range(50):
        done, status = os.waitpid(pid, os.WNOHANG)
        if done:
            exited = status
            break
        time.sleep(0.1)
    else:
        os.kill(pid, signal.SIGKILL)
        _, exited = os.waitpid(pid, 0)
    how = "still running at %.0fs, stopped by SIGTERM" % seconds
else:
    how = "EXITED ON ITS OWN after %.1fs" % (time.time() - start)
with open(os.path.join(out_dir, "exit.txt"), "w") as f:
    f.write("%s (wait status %s)\n" % (how, exited))
print(how)
