#!/usr/bin/env python3
"""Check that code/script comments are in English (no Cyrillic).

Enforces AGENTS.md common rule 6: comments in code and scripts must be in
English. Only comment text is scanned; string literals and data are ignored.
Project documentation (*.md) and the oracles (max-cpp-port, max-original,
max-original-dasm) are not scanned.

Approach: strip string literals from each line, then detect comment markers in
what remains. Multi-line strings (triple-quoted, here-strings) and block
comments are tracked across lines.

Usage:
    python check_comments_language.py [PATH ...]

Exit code: 0 = clean, 1 = at least one non-English comment found.
"""

import sys
import re
from pathlib import Path

CYRILLIC = re.compile(r"[\u0400-\u04FF\u0500-\u052F]")

EXTENSIONS = {".rs", ".sh", ".ps1", ".bat", ".py", ".toml", ".yml", ".yaml"}

SKIP_DIRS = {
    "max-cpp-port",
    "max-original",
    "max-original-dasm",
    "max-brh-docs",
    "max-brh-assets",
    "max-brh-vendor",
    ".git",
    "target",
    "node_modules",
    "__pycache__",
}

LINE_MARKERS = {".rs": "//", ".sh": "#", ".ps1": "#", ".py": "#",
                ".toml": "#", ".yml": "#", ".yaml": "#"}

BLOCK_COMMENTS = {".rs": [("/*", "*/")], ".ps1": [("<#", "#>")]}

# Multi-line string open/close delimiters per extension.
MULTILINE_STRINGS = {
    ".py": (('"""', '"""'), ("'''", "'''")),
    ".toml": (('"""', '"""'), ("'''", "'''")),
    ".ps1": (('@"', '"@'), ("@'", "'@")),
}


def _mask(line, start, end):
    """Return the line with [start, end) replaced by spaces."""
    return line[:start] + " " * (end - start) + line[end:]


def _strip_line_strings(ext, line):
    """Replace single-line string/char literals with spaces.

    Returns the stripped line and the string delimiter if a multi-line string
    is still open at end of line (else None).
    """
    i = 0
    n = len(line)
    out = list(line)
    while i < n:
        ch = line[i]

        # Multi-line string openers (triple-quote / here-string).
        for open_d, close_d in MULTILINE_STRINGS.get(ext, ()):
            if line.startswith(open_d, i):
                end = line.find(close_d, i + len(open_d))
                if end == -1:
                    for k in range(i, n):
                        out[k] = " "
                    return "".join(out), close_d
                for k in range(i, end + len(close_d)):
                    out[k] = " "
                i = end + len(close_d)
                break
        else:
            # Raw string r"..." / r'...'.
            if ch == "r" and i + 1 < n and line[i + 1] in ('"', "'") and ext in (".rs", ".py"):
                q = line[i + 1]
                j = i + 2
                while j < n and line[j] != q:
                    j += 1
                for k in range(i, min(j + 1, n)):
                    out[k] = " "
                i = j + 1
                continue

            # Double-quoted string.
            if ch == '"' and ext in (".rs", ".sh", ".ps1", ".py", ".toml", ".yml", ".yaml"):
                j = i + 1
                while j < n:
                    if line[j] == "\\" and j + 1 < n:
                        j += 2
                        continue
                    if line[j] == '"':
                        break
                    j += 1
                for k in range(i, min(j + 1, n)):
                    out[k] = " "
                i = j + 1
                continue

            # Single-quoted string / char literal.
            if ch == "'" and ext in (".rs", ".sh", ".ps1", ".py", ".toml", ".yml", ".yaml"):
                if ext == ".rs":
                    # Rust: 'a is a lifetime; 'x' is a char literal.
                    j = i + 1
                    if j < n and line[j] == "\\" and j + 1 < n:
                        j += 1
                    if j + 1 < n and line[j + 1] == "'":
                        for k in range(i, j + 2):
                            out[k] = " "
                        i = j + 2
                        continue
                    # lifetime — not a string
                    i += 1
                    continue
                j = i + 1
                while j < n:
                    if line[j] == "\\" and j + 1 < n:
                        j += 2
                        continue
                    if line[j] == "'":
                        break
                    j += 1
                for k in range(i, min(j + 1, n)):
                    out[k] = " "
                i = j + 1
                continue

            # Shell backtick substitution.
            if ch == "`" and ext == ".sh":
                j = line.find("`", i + 1)
                if j == -1:
                    j = n - 1
                for k in range(i, j + 1):
                    out[k] = " "
                i = j + 1
                continue

            i += 1

    return "".join(out), None


def _bat_line_comment(line):
    """Return (start, text) if the .bat line is a comment (:: or REM), else None."""
    stripped = line.lstrip(" \t")
    lead = len(line) - len(stripped)
    if stripped.startswith("::"):
        return lead, line[lead:]
    if stripped.startswith("REM"):
        return lead, line[lead:]
    return None


def _extract_comments(ext, text):
    """Yield (1-based line number, comment fragment)."""
    in_block = None  # block-comment close delimiter
    in_ml_string = None  # multi-line string close delimiter

    for lineno, line in enumerate(text.splitlines(), 1):
        # Continue a block comment from a previous line.
        if in_block is not None:
            end = line.find(in_block)
            if end == -1:
                yield lineno, line
                continue
            yield lineno, line[:end]
            line = " " * (end + len(in_block)) + line[end + len(in_block):]
            in_block = None

        # Continue a multi-line string from a previous line.
        if in_ml_string is not None:
            end = line.find(in_ml_string)
            if end == -1:
                continue  # whole line is still inside the string
            line = " " * (end + len(in_ml_string)) + line[end + len(in_ml_string):]
            in_ml_string = None

        stripped, open_ml = _strip_line_strings(ext, line)
        if open_ml is not None:
            in_ml_string = open_ml

        # .bat comments.
        if ext == ".bat":
            bc = _bat_line_comment(stripped)
            if bc:
                yield lineno, bc[1]
                continue

        marker = LINE_MARKERS.get(ext)
        i = 0
        n = len(stripped)
        while i < n:
            # Block comment open.
            handled = False
            for s, e in BLOCK_COMMENTS.get(ext, ()):
                if stripped.startswith(s, i):
                    end = stripped.find(e, i + len(s))
                    if end == -1:
                        yield lineno, stripped[i + len(s):]
                        in_block = e
                        i = n
                    else:
                        yield lineno, stripped[i + len(s):end]
                        i = end + len(e)
                    handled = True
                    break
            if handled:
                continue

            # Line comment.
            if marker is not None and stripped.startswith(marker, i):
                yield lineno, stripped[i:]
                break

            i += 1


def _strip_comment_quotes(frag):
    """Remove quoted/backtick text inside a comment (data references, not prose)."""
    out = []
    i = 0
    n = len(frag)
    while i < n:
        ch = frag[i]
        if ch in ('"', "'", "`"):
            j = i + 1
            while j < n and frag[j] != ch:
                if frag[j] == "\\" and j + 1 < n:
                    j += 1
                j += 1
            out.append(" " * (j - i + 1))
            i = j + 1
        else:
            out.append(ch)
            i += 1
    return "".join(out)


def check_file(path):
    """Return a list of (line_number, fragment) with Cyrillic comments."""
    ext = path.suffix.lower()
    if ext not in EXTENSIONS:
        return []
    try:
        text = path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return []
    out = []
    for lineno, frag in _extract_comments(ext, text):
        if CYRILLIC.search(_strip_comment_quotes(frag)):
            out.append((lineno, frag.strip()))
    return out


def main(argv):
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

    root = Path(__file__).resolve().parents[2]
    paths = [Path(p) for p in argv] if argv else [root]

    files = []
    for path in paths:
        if path.is_file():
            files.append(path)
        elif path.is_dir():
            for p in path.rglob("*"):
                if not p.is_file() or p.suffix.lower() not in EXTENSIONS:
                    continue
                try:
                    rel = p.relative_to(root)
                except ValueError:
                    rel = p
                if any(part in SKIP_DIRS for part in rel.parts):
                    continue
                files.append(p)

    violations = []
    for f in sorted(set(files)):
        for lineno, frag in check_file(f):
            try:
                rel = f.relative_to(root)
            except ValueError:
                rel = f
            violations.append(f"{rel}:{lineno}: {frag[:100]}")

    if violations:
        print("Non-English comments found:")
        for v in violations:
            print("  " + v)
        print(f"Total: {len(violations)} violation(s) in {len(files)} scanned file(s).")
        return 1

    print(f"OK: no non-English comments in {len(files)} scanned file(s).")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
