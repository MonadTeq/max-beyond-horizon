#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Драйвер отладчика DOSBox-X через PTY.

Дебаггер DOSBox-X — ncurses-интерфейс: запускаем в псевдотерминале, подаём
команды построчно, захватываем вывод.

Язык команд (cmds-файл):
- обычная строка — команда отладчика (BP/BPINT/RUN/GDT/...);
- «WAIT <seconds>» — пауза перед следующей командой;
- пустая строка — Enter (для пейджера);
- строки с «#» — комментарии.

Использование (в контейнере max-dosbox):
    python3 re_drive.py cmds.txt dosbox-out.txt [timeout_sec=900]
"""

import fcntl
import os
import select
import struct
import sys
import termios
import time

HERE = os.path.dirname(os.path.abspath(__file__))
CONF = os.path.join(HERE, "maxrun-debug.conf")
DOSBOX = os.path.join(HERE, "..", "dosbox-x", "src", "dosbox-x")
GAME_DIR = "/src/max-original"


def run(cmds_file, out_file, timeout=900):
    commands = [c.strip() for c in open(cmds_file).read().splitlines() if not c.strip().startswith("#")]
    env = dict(os.environ)
    env["SDL_VIDEODRIVER"] = "dummy"
    env["SDL_AUDIODRIVER"] = "dummy"
    env["TERM"] = "xterm-256color"

    pid, fd = os.forkpty()
    if pid == 0:
        os.chdir(GAME_DIR)
        os.execvpe(DOSBOX, [DOSBOX, "-conf", CONF], env)

    # Размер псевдотерминала.
    fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("HHHH", 50, 132, 0, 0))

    out = open(out_file, "wb")
    start = time.time()
    next_cmd = time.time() + 20.0  # DOSBox-X поднимается и доходит до DEBUGBOX
    idx = 0
    while time.time() - start < timeout:
        r, _, _ = select.select([fd], [], [], 1.0)
        if r:
            try:
                data = os.read(fd, 65536)
            except OSError:
                break
            if not data:
                break
            out.write(data)
            out.flush()
        if idx < len(commands) and time.time() >= next_cmd:
            cmd = commands[idx]
            try:
                if cmd.startswith("WAIT "):
                    delay = float(cmd.split()[1])
                    next_cmd = time.time() + delay
                else:
                    os.write(fd, (cmd + "\n").encode())
                    next_cmd = time.time() + 2.5
            except OSError:
                break
            print(">> %s" % cmd, flush=True)
            idx += 1

    try:
        os.kill(pid, 9)
        os.waitpid(pid, 0)
    except OSError:
        pass
    os.close(fd)
    out.close()
    print("output written to %s" % out_file)


if __name__ == "__main__":
    cmds = sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "cmds_c.txt")
    out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, "dosbox-out.txt")
    limit = int(sys.argv[3]) if len(sys.argv) > 3 else 900
    run(cmds, out, limit)
