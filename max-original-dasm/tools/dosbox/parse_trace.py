#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Извлечение трассы остановов из сырого вывода отладчика DOSBox-X.

Вход: dosbox-out.txt — «сырой» захват ncurses-интерфейса (ANSI-последовательности,
перерисовки экрана, курсорные позиции). Полное восстановление экрана 50x132 не
делается: после снятия ANSI текст разбирается регулярными выражениями.

Выход: trace.csv (utf-8) со столбцами:
    stop_index, cs, eip, linear, mode, registers, code_line

- cs, eip      — из окна Register Overview (например 1048, 00002714);
- linear       — int(cs,16)*16 + int(eip,16), десятичное; только для реального
                 режима (в PM/Pr16/Pr32 не вычисляется и остаётся пустым);
- mode         — Pr16/Pr32, если в строке регистров есть такой маркер, иначе
                 Real (обычный реальный режим) либо пусто;
- registers    — нормализованная строка «EAX=.. ESI=.. DS=.. ES=.. FS=.. GS=..
                 SS=.. CS=.. EIP=..» из того же кадра регистров;
- code_line    — строка из окна Code Overview для этого CS:EIP
                 (вида «1048:00002714 EBDF jmp short 000026F5»).

Алгоритм:
1) снять ANSI (CSI, ESC(0/ESC(B, ESC=, ESC>, одиночные ESC);
2) найти все пары CS=... / EIP=... в порядке появления, схлопнуть подряд
   идущие одинаковые пары (экран перерисовывается многократно);
3) для каждого останова взять ближайшую строку кода из Code Overview;
4) записать CSV, вывести «stops: N» и путь.

Использование:
    python parse_trace.py [dosbox-out.txt] [trace.csv]
"""

import bisect
import csv
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_IN = os.path.join(HERE, "dosbox-out.txt")
DEFAULT_OUT = os.path.join(HERE, "trace.csv")

# --- снятие ANSI -----------------------------------------------------------

CSI_RE = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")        # ESC[ ... буква
CHARSET_RE = re.compile(r"\x1b[()][0-9A-B]")           # ESC(B / ESC(0
ESC2_RE = re.compile(r"\x1b[=>78]")                    # ESC=, ESC>, ESC7, ESC8
CTRL_RE = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x7f]")

# ESC[n b (REP) — повторить предыдущий символ n раз. DOSBox-X рисует так
# «хвосты» значений (EAX=0 <REP 6> 2  ->  EAX=00000002), поэтому REP нужно
# раскрыть до общей зачистки CSI, иначе теряются цифры регистров.
REP_RE = re.compile(r"\x1b\[([0-9]{0,4})b")

CS_RE = re.compile(r"CS\s*=\s*([0-9A-Fa-f]{4})(?![0-9A-Fa-f])")
EIP_RE = re.compile(r"EIP\s*=\s*([0-9A-Fa-f]{8})(?![0-9A-Fa-f])")

# CS и EIP в одном кадре регистров стоят рядом (между ними ~10 символов);
# ограничение не даёт склеить CS одного кадра с EIP следующего.
MAX_PAIR_GAP = 200

# --- разбор кода -----------------------------------------------------------

# Строка Code Overview: «1048:00002714 EBDF  jmp  short 000026F5».
# Адрес может стоять не в начале строки (заголовок окна и строка кода иногда
# рисуются одним write-ом без перевода строки), поэтому ^ не требуется.
CODE_RE = re.compile(
    r"([0-9A-F]{4}):([0-9A-F]{8})"
    r"[ \t]+((?:[0-9A-F]{2})(?:[ \t]*[0-9A-F]{2})*)"
    r"[ \t]+(\S.*?)[ \t]*$",
    re.M,
)

SECTION_MARKERS = ("Register Overview", "Data view",
                   "Code Overview", "Output")

FIELD_RE = re.compile(
    r"\b(EAX|ESI|DS|ES|FS|GS|SS|CS|EIP)\s*=\s*([0-9A-Fa-f]{1,8})\b"
)
REG_FIELDS = ("EAX", "ESI", "DS", "ES", "FS", "GS", "SS", "CS", "EIP")


def _expand_rep(m):
    """Раскрыть ESC[nb в n копий предыдущего печатного символа."""
    n = int(m.group(1) or "1")
    prev = m.string[m.start() - 1] if m.start() > 0 else ""
    if n <= 0 or n > 500 or not prev or prev == "\x1b" or prev in "\r\n":
        return " "
    return prev * n


def strip_ansi(raw):
    """Заменить управляющие последовательности пробелами, оставив текст/строки."""
    s = REP_RE.sub(_expand_rep, raw)
    s = CSI_RE.sub(" ", s)
    s = CHARSET_RE.sub(" ", s)
    s = ESC2_RE.sub(" ", s)
    s = s.replace("\x1b", " ")          # «сиротские» ESC в LOG-префиксе
    s = CTRL_RE.sub(" ", s)
    s = s.replace("\r\n", "\n").replace("\r", "\n")
    return s


def find_stops(text):
    """Пары CS:EIP в порядке появления, подряд идущие дубликаты схлопнуты."""
    cs_matches = list(CS_RE.finditer(text))
    eip_matches = list(EIP_RE.finditer(text))
    stops = []
    j = 0
    for cm in cs_matches:
        cs = cm.group(1).upper()
        while j < len(eip_matches) and eip_matches[j].start() < cm.end():
            j += 1
        if j >= len(eip_matches):
            break
        em = eip_matches[j]
        if em.start() - cm.end() > MAX_PAIR_GAP:
            continue
        eip = em.group(1).upper()
        if stops and stops[-1]["cs"] == cs and stops[-1]["eip"] == eip:
            continue
        stops.append({"cs": cs, "eip": eip, "pos": em.start()})
    return stops


def marker_list(text):
    marks = []
    for name in SECTION_MARKERS:
        for m in re.finditer(re.escape(name), text):
            marks.append((m.start(), name))
    marks.sort(key=lambda t: t[0])
    return marks


def find_code_lines(text, marks):
    """Строки кода из окон Code Overview: (pos, cs, eip, bytes, asm)."""
    positions = [p for p, _ in marks]
    out = []
    for m in CODE_RE.finditer(text):
        pos = m.start()
        i = bisect.bisect_right(positions, pos) - 1
        section = marks[i][1] if i >= 0 else None
        if section != "Code Overview":
            continue
        code_bytes = m.group(3)
        if len(re.findall(r"[0-9A-F]{2}", code_bytes)) > 8:
            continue
        out.append((pos, m.group(1), m.group(2), code_bytes, m.group(4)))
    return out


def register_window(text, eip_pos):
    """Кусок текста с окном Register Overview, содержащим данный EIP."""
    start = text.rfind("EAX=", max(0, eip_pos - 800), eip_pos)
    win = ""
    if start != -1:
        win = text[start:start + 1000]
        cut = len(win)
        for marker in SECTION_MARKERS:
            k = win.find(marker, 10)
            if k != -1:
                cut = min(cut, k)
        win = win[:cut]
    if "EIP" not in win:
        win = text[max(0, eip_pos - 300):eip_pos + 300]
    return win


def parse_registers(text, eip_pos, cs, eip):
    """(registers, mode) из кадра регистров, в котором находится останов."""
    win = register_window(text, eip_pos)
    values = {}
    for m in FIELD_RE.finditer(win):
        name = m.group(1)
        if name not in values:
            values[name] = m.group(2).upper()
    if values.get("EIP") != eip:
        # окно не соответствует останову (неполная перерисовка) — не берём
        # из него чужие значения
        values = {}
        mode = ""
    elif "Pr16" in win:
        mode = "Pr16"
    elif "Pr32" in win:
        mode = "Pr32"
    elif "Real" in win:
        mode = "Real"
    else:
        mode = ""
    values["CS"] = cs
    values["EIP"] = eip
    registers = " ".join(
        "%s=%s" % (name, values.get(name, "?")) for name in REG_FIELDS
    )
    return registers, mode


def pick_code_line(code_lines, cs, eip, pos):
    best = None
    for cand in code_lines:
        if cand[1] != cs or cand[2] != eip:
            continue
        dist = abs(cand[0] - pos)
        if best is None or dist < best[0]:
            best = (dist, cand)
    if best is None:
        return ""
    cand = best[1]
    code_bytes = re.sub(r"\s+", " ", cand[3]).strip()
    asm = re.sub(r"\s+", " ", cand[4]).strip()
    return "%s:%s %s %s" % (cs, eip, code_bytes, asm)


def parse(raw):
    text = strip_ansi(raw)
    stops = find_stops(text)
    marks = marker_list(text)
    code_lines = find_code_lines(text, marks)

    rows = []
    for idx, stop in enumerate(stops):
        cs, eip = stop["cs"], stop["eip"]
        registers, mode = parse_registers(text, stop["pos"], cs, eip)
        if mode in ("Pr16", "Pr32"):
            linear = ""
        else:
            linear = int(cs, 16) * 16 + int(eip, 16)
        rows.append([
            idx,
            cs,
            eip,
            linear,
            mode,
            registers,
            pick_code_line(code_lines, cs, eip, stop["pos"]),
        ])
    return rows


def main(argv):
    in_path = argv[1] if len(argv) > 1 else DEFAULT_IN
    out_path = argv[2] if len(argv) > 2 else DEFAULT_OUT

    if not os.path.isfile(in_path):
        print("input not found: %s" % in_path, file=sys.stderr)
        return 2

    with open(in_path, encoding="utf-8", errors="replace") as fh:
        raw = fh.read()

    rows = parse(raw)

    with open(out_path, "w", encoding="utf-8", newline="") as fh:
        writer = csv.writer(fh, lineterminator="\n")
        writer.writerow(["stop_index", "cs", "eip", "linear", "mode",
                         "registers", "code_line"])
        writer.writerows(rows)

    print("stops: %d" % len(rows))
    print("trace: %s" % os.path.abspath(out_path))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
