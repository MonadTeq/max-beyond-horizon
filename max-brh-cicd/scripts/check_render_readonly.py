#!/usr/bin/env python3
"""Guardrail: the render/UI layer reads the world, it never mutates it (Stage A, A2).

The invariant is fixed by `max-brh-docs/development/sdl-ui-integration.md` section 3
(Stage A, A2): rendering, panels, overlays, the HUD, the cursor and the build-site
ghost observe the match through `StateRef` / `&App` only. They never hold
`&mut App`, `&mut GameUniverse`, `&mut GameState` or a mutable state view: such an
access would be a second, wide channel around the command path and a determinism
hole (a local mutation that a remote client never sees).

Checked modules (the render/UI surface):

    crates/max_app/src/render.rs      map/unit drawing
    crates/max_app/src/panels.rs      bottom panels
    crates/max_app/src/overlays.rs    GRID/HITS/AMMO/... overlays
    crates/max_app/src/hud.rs         HUD bar, message log, minimap, path preview
    crates/max_app/src/cursor.rs      context cursor selection
    crates/max_app/src/buildsite.rs   build-site ghost

Rules (code only: comments, string/char literals and `#[cfg(test)]` modules are
skipped):

R1  no mutable facade reference anywhere in the module вЂ” `&mut App`,
    `&mut GameUniverse`, `&mut GameState`, `&mut StateRef`, `&mut StateDelta` вЂ”
    and no `state_mut()` accessor;
R2  a drawing function (name starts with draw/render/blit/paint/stroke/plot/fill,
    or it takes a `&mut RgbaBuffer` / `&mut [u8]` output buffer) must not have a
    mutable `App`/`GameUniverse` receiver (`&mut self` inside such an impl block);
R3  a drawing function must not call a simulation mutator (`submit`, `tick`,
    `apply_now`, `apply_barrier`, `apply_state_sync`, `end_local_turn`, ...).

Usage:
    python check_render_readonly.py [PATH ...]   # default: the six modules above
    python check_render_readonly.py --self-test  # prove the detector still fires

Exit code: 0 = clean, 1 = violations found, 2 = usage error / missing file.
"""

import re
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# The render/UI surface of the client (Stage A, A2). Add a module here when a new
# drawing module appears; the list is the guardrail's whole scope.
RENDER_MODULES = [
    "max-brh/crates/max_app/src/render.rs",
    "max-brh/crates/max_app/src/panels.rs",
    "max-brh/crates/max_app/src/overlays.rs",
    "max-brh/crates/max_app/src/hud.rs",
    "max-brh/crates/max_app/src/cursor.rs",
    "max-brh/crates/max_app/src/buildsite.rs",
]

# R1: a mutable reference to the facade or to a state view. Only the type matters,
# so the pattern covers `App`, `App<D>`, `crate::App<D>`, `GameUniverse<D>`, ....
MUTABLE_FACADE = [
    (re.compile(r"&\s*mut\s+(?:crate::|max_app::|max_core::(?:universe|game)::)?App\b"),
     "&mut App"),
    (re.compile(r"&\s*mut\s+(?:crate::|crate::universe::|max_core::universe::)?GameUniverse\b"),
     "&mut GameUniverse"),
    (re.compile(r"&\s*mut\s+(?:crate::|crate::game::|max_core::game::)?GameState\b"),
     "&mut GameState"),
    (re.compile(r"&\s*mut\s+(?:crate::universe::|max_core::universe::)?StateRef\b"),
     "&mut StateRef"),
    (re.compile(r"&\s*mut\s+(?:crate::universe::|max_core::universe::)?StateDelta\b"),
     "&mut StateDelta"),
]

# The only mutable accessor of the simulation on the facade (`hud_mut` and other
# UI-state accessors are fine and are not listed).
MUTABLE_ACCESSOR = re.compile(r"\bstate_mut\s*\(")

# R3: operations that move or mutate the simulation. A renderer that calls one of
# them is on the command path, not on the observation path.
SIMULATION_MUTATORS = [
    "submit",
    "submit_remote",
    "tick",
    "apply_now",
    "apply_barrier",
    "apply_state_sync",
    "apply_command",
    "flush_commands",
    "drain_outgoing",
    "end_local_turn",
    "begin_local_round",
    "resume_local_round",
    "reset_world",
    "load_state",
    "spawn_unit",
    "destroy_unit",
    "write_unit",
    "eliminate_team",
    "set_game_state",
    "finish_turn",
    "step_begin_turn_barrier",
    "step_end_turn_barrier",
    "step_crc_barrier",
    "unit_mut",
    "map_mut",
    "team_mut",
    "teams_mut",
    "team_units_mut",
    "rng_mut",
    "ai_mut",
    "complex_mut",
    "dispatcher_mut",
]
MUTATOR_CALL = re.compile(r"\b(" + "|".join(re.escape(m) for m in SIMULATION_MUTATORS) + r")\s*\(")

# A drawing function: by name, or because it paints into a frame buffer.
DRAW_PREFIXES = ("draw", "render", "blit", "paint", "stroke", "plot", "fill")
DRAW_BUFFER = re.compile(r"&\s*mut\s+(?:max_render::)?RgbaBuffer\b|&\s*mut\s*\[u8\]")

FN_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:unsafe\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)"
)
IMPL_RE = re.compile(r"^\s*(?:unsafe\s+)?impl\b")
TEST_MOD_RE = re.compile(r"^\s*mod\s+tests\s*\{")
CFG_TEST_RE = re.compile(r"^\s*#\[cfg\(test\)\]\s*$")


def _blank(out, start, end):
    """Blank out [start, end) of a character list, keeping newlines."""
    for index in range(start, end):
        if out[index] != "\n":
            out[index] = " "


def _raw_string_hashes(text, index):
    """Return the raw-string hash prefix if a raw string starts at `index`."""
    match = re.match(r'(?:b|c)?r(#*)"', text[index:index + 24])
    if not match:
        return None
    if index > 0 and (text[index - 1].isalnum() or text[index - 1] == "_"):
        return None
    return match.group(1)


def strip_code(text):
    """Blank comments and string/char literals; keep code, braces and newlines.

    Working on masked text is what makes the checks honest: doc comments in these
    modules talk about `&mut` and about packet numbers, strings contain `{`/`}`,
    and none of that may count as code.
    """
    out = list(text)
    i = 0
    n = len(text)
    while i < n:
        ch = text[i]
        nxt = text[i + 1] if i + 1 < n else ""

        # Line comment.
        if ch == "/" and nxt == "/":
            end = text.find("\n", i)
            if end == -1:
                end = n
            _blank(out, i, end)
            i = end
            continue

        # Block comment (`/* */`, nested in Rust).
        if ch == "/" and nxt == "*":
            depth = 1
            j = i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth += 1
                    j += 2
                elif text.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            _blank(out, i, j)
            i = j
            continue

        # Raw string: r"..." / r#"..."# / br"..." / br#"..."#.
        hashes = _raw_string_hashes(text, i)
        if hashes is not None:
            open_quote = text.index('"', i)
            close = '"' + hashes
            end = text.find(close, open_quote + 1)
            end = n if end == -1 else end + len(close)
            _blank(out, i, end)
            i = end
            continue

        # String literal (escapes included).
        if ch == '"':
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            _blank(out, i, j)
            i = j
            continue

        # Char literal or lifetime.
        if ch == "'":
            j = i + 1
            if j < n and text[j] == "\\":
                k = j + 1
                while k < n and text[k] != "'":
                    k += 1
                _blank(out, i, k + 1)
                i = k + 1
                continue
            if j + 1 < n and text[j + 1] == "'":
                _blank(out, i, j + 2)
                i = j + 2
                continue
            i += 1
            continue

        i += 1

    return "".join(out)


def _test_module_lines(lines):
    """Line indexes of every `#[cfg(test)] mod tests { ... }` block."""
    excluded = set()
    for index, line in enumerate(lines):
        if not TEST_MOD_RE.match(line):
            continue
        previous = index - 1
        while previous >= 0 and not lines[previous].strip():
            previous -= 1
        if previous < 0 or not CFG_TEST_RE.match(lines[previous]):
            continue
        depth = 0
        cursor = index
        while cursor < len(lines):
            depth += lines[cursor].count("{") - lines[cursor].count("}")
            excluded.add(cursor)
            if depth == 0:
                break
            cursor += 1
    return excluded


def _impl_blocks(lines, excluded):
    """Map line index -> impl target text (the part after `impl`)."""
    owner = [None] * len(lines)
    index = 0
    while index < len(lines):
        if index in excluded or not IMPL_RE.match(lines[index]):
            index += 1
            continue
        header = ""
        cursor = index
        while cursor < len(lines) and "{" not in lines[cursor]:
            header += " " + lines[cursor]
            cursor += 1
        if cursor < len(lines):
            header += " " + lines[cursor].split("{", 1)[0]
        target = header.split("impl", 1)[1] if "impl" in header else ""
        depth = 0
        end = cursor
        while end < len(lines):
            depth += lines[end].count("{") - lines[end].count("}")
            owner[end] = target
            if depth <= 0:
                break
            end += 1
        index = end + 1
    return owner


def _functions(lines, excluded, owner):
    """Collect functions of the live code: name, line, signature, body, impl owner."""
    functions = []
    index = 0
    while index < len(lines):
        if index in excluded:
            index += 1
            continue
        match = FN_RE.match(lines[index])
        if not match:
            index += 1
            continue

        signature = ""
        cursor = index
        while cursor < len(lines):
            signature += " " + lines[cursor]
            if "{" in lines[cursor] or ";" in lines[cursor]:
                break
            cursor += 1

        body_start = cursor
        depth = 0
        end = cursor
        while end < len(lines):
            depth += lines[end].count("{") - lines[end].count("}")
            if depth <= 0:
                break
            end += 1

        params = signature.split("{", 1)[0]
        body = "\n".join(lines[body_start:end + 1])
        functions.append({
            "name": match.group(1),
            "line": index + 1,
            "params": params,
            "body": body,
            "impl": owner[index],
        })
        index = end + 1
    return functions


def _is_drawing(function):
    if function["name"].startswith(DRAW_PREFIXES):
        return True
    return bool(DRAW_BUFFER.search(function["params"]))


def analyse(text):
    """Return (violations, stats) for one Rust source text."""
    masked = strip_code(text)
    lines = masked.splitlines()
    excluded = _test_module_lines(lines)
    owner = _impl_blocks(lines, excluded)
    functions = _functions(lines, excluded, owner)

    # Live text: test modules blanked, so R1 sees code only and still knows line numbers.
    live_lines = ["" if index in excluded else line for index, line in enumerate(lines)]
    live = "\n".join(live_lines)

    violations = []

    # R1: a mutable facade reference in the module.
    for pattern, what in MUTABLE_FACADE:
        for match in pattern.finditer(live):
            line = live.count("\n", 0, match.start()) + 1
            violations.append((line, "R1", f"mutable facade reference `{what}`"))
    for match in MUTABLE_ACCESSOR.finditer(live):
        line = live.count("\n", 0, match.start()) + 1
        violations.append((line, "R1", "`state_mut()` accessor in a render module"))

    drawing = 0
    for function in functions:
        if not _is_drawing(function):
            continue
        drawing += 1

        # R2: a drawing method with a mutable App/GameUniverse receiver.
        target = function["impl"] or ""
        facade_impl = re.search(r"\b(?:App|GameUniverse)\b", target) is not None
        if facade_impl and re.search(r"&\s*mut\s+self\b", function["params"]):
            violations.append((
                function["line"], "R2",
                f"drawing function `{function['name']}` takes `&mut self` on the facade",
            ))

        # R3: a drawing function that moves the simulation.
        for match in MUTATOR_CALL.finditer(function["body"]):
            offset = function["body"].count("\n", 0, match.start())
            violations.append((
                function["line"] + offset, "R3",
                f"drawing function `{function['name']}` calls `{match.group(1)}`",
            ))

    stats = {
        "functions": len(functions),
        "drawing": drawing,
    }
    return violations, stats


def check_file(path):
    text = path.read_text(encoding="utf-8", errors="replace")
    return analyse(text)


SELF_TEST_CASES = {
    "mutable_facade_parameter": (
        "pub fn render_frame(app: &mut App) -> u32 { 0 }\n"
        "impl<D: Dispatcher> App<D> {\n"
        "    fn draw_units(&self, universe: &mut GameUniverse<D>) -> u32 { 0 }\n"
        "}\n"
    ),
    "mutable_receiver_in_app_impl": (
        "impl<D: Dispatcher> App<D> {\n"
        "    pub fn draw_units(&mut self, frame: &mut RgbaBuffer) -> u32 { 0 }\n"
        "}\n"
    ),
    "simulation_mutation_in_renderer": (
        "pub fn draw_units(app: &App, frame: &mut RgbaBuffer) {\n"
        "    app.universe.apply_now(Command::FinishedTurn(0));\n"
        "}\n"
    ),
    "clean_renderer": (
        "pub fn render_hud(buffer: &mut RgbaBuffer, view: &HudView<'_>) {\n"
        "    let state = view.state;\n"
        "    let _ = state.player_team();\n"
        "}\n"
        "impl<D: Dispatcher> App<D> {\n"
        "    pub fn draw_unit_menu(&self, frame: &mut RgbaBuffer) {}\n"
        "}\n"
    ),
}


def self_test():
    """Prove the detector still fires on synthetic violations and stays quiet on clean code."""
    ok = True
    with tempfile.TemporaryDirectory() as tmp:
        for name, source in SELF_TEST_CASES.items():
            path = Path(tmp) / f"{name}.rs"
            path.write_text(source, encoding="utf-8")
            violations, _stats = check_file(path)
            expected = name != "clean_renderer"
            state = "fired" if violations else "silent"
            if bool(violations) != expected:
                ok = False
                print(f"FAIL self-test {name}: expected "
                      f"{'a violation' if expected else 'no violation'}, detector {state}")
            else:
                print(f"ok   self-test {name}: {len(violations)} violation(s), detector {state}")
    return 0 if ok else 1


def main(argv):
    try:
        sys.stdout.reconfigure(encoding="utf-8", errors="replace")
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    except Exception:
        pass

    if argv and argv[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    if argv and argv[0] == "--self-test":
        return self_test()

    paths = [Path(p) for p in argv] if argv else [ROOT / p for p in RENDER_MODULES]

    violations = []
    total_functions = 0
    total_drawing = 0
    scanned = 0

    for path in paths:
        if not path.is_file():
            print(f"check_render_readonly: file not found: {path}", file=sys.stderr)
            return 2
        scanned += 1
        found, stats = check_file(path)
        total_functions += stats["functions"]
        total_drawing += stats["drawing"]
        try:
            label = path.resolve().relative_to(ROOT)
        except ValueError:
            label = path
        for line, rule, message in found:
            violations.append(f"{label}:{line}: {rule}: {message}")

    if violations:
        print("Render layer is not read-only:")
        for violation in violations:
            print("  " + violation)
        print(f"Total: {len(violations)} violation(s) in {scanned} scanned module(s).")
        return 1

    print(
        f"OK: render layer is read-only in {scanned} module(s) "
        f"({total_functions} function(s), {total_drawing} drawing function(s), "
        f"0 mutable facade reference)."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

