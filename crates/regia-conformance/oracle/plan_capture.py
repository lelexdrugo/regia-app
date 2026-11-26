"""Dev only: the vectors of `plan` (#444), captured from the prototype.

Two sources, both answered by the prototype as it is:

- its own unit tests, `tools/tests/test_piano_mermaid.py` of the dossier (#239): every class runs with the functions
  of the plan wrapped, so that each outermost call is recorded with its arguments and its answer. The functions are
  the graph (`piano_grafo`), the diagram (`piano_mermaid`), its labels (`_piano_titolo`, `_piano_stato`) and its
  notes (`piano_note`). The tree and the line before the diagram (`piano_albero`, `piano_markdown`) are text for
  people, which the product writes from its catalog: the fixture cases compare them;
- graphs drawn at random from a fixed seed, each with its graph and its diagram.

A task is written in the product's words: its status and maturity by their canonical ids, from the v1 format of
`regia-core` (`tracker_v1.toml`), and the keys that have their default value are left out. Its measures in the
graph are written only where the call gets a single task (`_piano_stato`): elsewhere the product computes them
from all the tasks of the call, as the prototype did. The tests' data are synthetic; one project's name, which
could be a real one, is changed. `tests/plan.rs` replays every vector on the port.

    python -I plan_capture.py --dossier <dossier> --out ../tests/plan/vectors.json
"""
import argparse
import functools
import importlib
import inspect
import io
import json
import random
import sys
import tomllib
import unittest
from pathlib import Path

sys.dont_write_bytecode = True

HERE = Path(__file__).resolve().parent
FORMAT = HERE.parent.parent / "regia-core" / "formats" / "tracker_v1.toml"

# The prototype's functions the port answers for, by the name the vectors give them.
FUNCTIONS = {"piano_grafo": "graph", "piano_mermaid": "diagram", "_piano_titolo": "title", "_piano_stato": "status",
             "piano_note": "notes"}

# A name in the prototype's tests that could be the owner's, and the one the vectors use instead.
SYNTHETIC = (("paper-delphi-x", "paper-y"),)

# The keys of a task left out when they have these values: those of the tests' own helper `t`.
DEFAULTS = {"state": "OPEN", "status": "approved", "project": "p", "maturity": "ready", "objective": False,
            "parent": None, "sub": [], "blocked_by": []}

# Titles the random graphs draw from: blanks, quotes, angles, a number before `;`, prefixes, a long one, non-ASCII.
TITLES = ('Il "resta" <b>', "Vedi #12; e #7", "[#138 B] Campo", "[#138.4b] Piano", "[#138 A, seguito] Hook", "[p] Scouting",
          "[q] Altro", "  spazi\tvari qui  ", "x" * 50, "àèìòù ✋ emoji 🧪", "corto", "#3;#4;", "<<>>", "a" * 37 + " b",
          "prefisso\u001fseparatore")


def synthetic(text):
    for real, fake in SYNTHETIC:
        text = text.replace(real, fake)
    return text


class Words:
    """The prototype's words of a status and of a maturity, and the product's ids, from the v1 format."""

    def __init__(self):
        v1 = tomllib.loads(FORMAT.read_text(encoding="utf-8"))
        self.status = dict(v1["status_labels"])
        self.maturity = {label.split(":", 1)[1]: id for id, label in v1["labels"]["maturity"].items()}

    def task(self, t, measures=False):
        out = {"number": t["number"], "title": synthetic(t["title"] or "")}
        values = {"state": t.get("state", "OPEN"), "status": self.status.get(t.get("stato")) if t.get("stato") else None,
                  "project": synthetic(t["progetto"]) if t.get("progetto") is not None else None,
                  "maturity": self.maturity.get(t.get("maturita")) if t.get("maturita") else None,
                  "objective": bool(t.get("obiettivo")), "parent": t.get("parent"),
                  "sub": [[n, s] for n, s in t.get("sub") or ()],
                  "blocked_by": [[n, s] for n, s in t.get("bloccato_da") or ()]}
        assert t.get("stato") is None or values["status"], t.get("stato")
        assert t.get("maturita") is None or values["maturity"], t.get("maturita")
        out.update({k: v for k, v in values.items() if v != DEFAULTS[k]})
        total = t.get("bloccato_da_totale", len(values["blocked_by"]))
        if total != len(values["blocked_by"]):
            out["blocked_by_total"] = total
        if measures:
            out.update({"unblocks": t.get("sblocca"), "unblocks_total": t.get("sblocca_totale"), "chain": t.get("catena"),
                        "cycle": bool(t.get("ciclo"))})
        return out

    def tasks(self, d):
        return [self.task(d[n]) for n in sorted(d)]


def graph(g):
    """`piano_grafo`'s answer in the product's keys, each set as a sorted list."""
    return {"nodes": sorted(g["nodi"]), "external": sorted(g["esterni"]), "main_line": list(g["linea"]),
            "boxes": sorted([c, p] for c, p in g["cont"].items()), "edges": [list(e) for e in g["archi"]],
            "nested": [list(e) for e in g["annidati"]], "edges_lost": g["archi_persi"], "inbox_hidden": list(g["inbox_fuori"]),
            "truncated": list(g["tagliati"]), "root": g["radice"], "project": synthetic(g["progetto"]) if g["progetto"] else g["progetto"]}


def vector(words, name, arguments, answer):
    a = arguments
    if name == "graph":
        return {"args": {"tasks": words.tasks(a["d"]), "root": a["radice"], "project": synthetic(a["progetto"]) if a["progetto"] else a["progetto"],
                         "max_nodes": a["max_nodi"]}, "out": graph(answer)}
    if name == "diagram":
        return {"args": {"tasks": words.tasks(a["d"]), "graph": graph(a["g"]), "command": a["comando"], "date": a["data"]},
                "out": synthetic(answer)}
    if name == "title":
        return {"args": {"task": words.task(a["t"]), "width": a["lung"], "escaped": a["mermaid"]}, "out": synthetic(answer)}
    if name == "status":
        return {"args": {"task": words.task(a["t"], measures=True), "external": a["esterno"]}, "out": synthetic(answer)}
    if name == "notes":
        return {"args": {"graph": graph(a["g"]), "root_hint": a["per_progetto"]}, "out": [synthetic(n) for n in answer]}
    raise ValueError(name)


def from_the_tests(regia, tests, words):
    calls, depth = [], [0]

    def wrap(name, fn):
        signature = inspect.signature(fn)

        @functools.wraps(fn)
        def recorded(*args, **kwargs):
            outer = depth[0] == 0
            depth[0] += 1
            try:
                answer = fn(*args, **kwargs)
            finally:
                depth[0] -= 1
            if outer:
                bound = signature.bind(*args, **kwargs)
                bound.apply_defaults()
                calls.append({"fn": FUNCTIONS[name], **vector(words, FUNCTIONS[name], dict(bound.arguments), answer)})
            return answer
        return recorded

    for name in FUNCTIONS:
        setattr(regia, name, wrap(name, getattr(regia, name)))
    module = importlib.import_module(tests)
    suite = unittest.TestLoader().loadTestsFromModule(module)
    result = unittest.TextTestRunner(stream=io.StringIO(), verbosity=0).run(suite)
    if not result.wasSuccessful():
        for test, trace in result.failures + result.errors:
            print(test, trace, file=sys.stderr)
        sys.exit("the prototype's tests did not pass: no vectors")
    seen, out = set(), []
    for call in calls:
        key = json.dumps(call, sort_keys=True, ensure_ascii=False)
        if key not in seen:
            seen.add(key)
            out.append(call)
    return out, result.testsRun


def at_random(regia, words, count, seed):
    """`count` graphs of a few tasks each, in two projects, with parents, blockers, closed issues, Objectives,
    statuses, maturities, cycles and blockers read in part; each with a root or a project, and a cap."""
    rnd = random.Random(seed)
    statuses = (None, "Inbox", "Proposta", "Approvato", "Approvato", "In corso", "Decisione", "Fatto", "Scartato")
    out = []
    for _ in range(count):
        numbers = rnd.sample(range(1, 80), rnd.randint(2, 14))
        state = {n: "CLOSED" if rnd.random() < 0.12 else "OPEN" for n in numbers}
        tasks = []
        for i, n in enumerate(numbers):
            blockers = [b for b in rnd.sample(numbers, rnd.randint(0, min(3, len(numbers)))) if b != n]
            tasks.append({
                "number": n, "title": rnd.choice(TITLES + (f"t{n}",) * 6), "state": state[n], "stateReason": None,
                "stato": rnd.choice(statuses), "progetto": rnd.choice(("p", "p", "p", "q", None)),
                "maturita": rnd.choice(("pronto", "pronto", "abbozzo", None)), "obiettivo": rnd.random() < 0.08,
                "labels": [], "parent": rnd.choice(numbers[:i]) if i and rnd.random() < 0.45 else None, "sub": [],
                "bloccato_da": [(b, state[b]) for b in blockers], "url": "", "updatedAt": "", "createdAt": None,
                "closedAt": None, "body": ""})
            if blockers and rnd.random() < 0.06:
                tasks[-1]["bloccato_da_totale"] = len(blockers) + 1
            else:
                tasks[-1]["bloccato_da_totale"] = len(blockers)
        for t in tasks:
            padre = next((p for p in tasks if p["number"] == t["parent"]), None)
            if padre is not None and rnd.random() < 0.85:
                padre["sub"].append((t["number"], t["state"]))
        grafo = regia.ordine_grafo(tasks)
        d = {t["number"]: regia.con_grafo(t, grafo) for t in tasks}
        root = rnd.choice(numbers) if rnd.random() < 0.5 else None
        project = None if root is not None else rnd.choice(("p", "q"))
        cap = rnd.choice((1, 2, 3, 5, 8, 25))
        g = regia.piano_grafo(d, radice=root, progetto=project, max_nodi=cap)
        command = regia.piano_comando(root, project, mermaid=True, max_nodi=cap)
        out.append({"fn": "random", "args": {"tasks": words.tasks(d), "root": root, "project": project, "max_nodes": cap,
                                             "command": command, "date": "2026-01-10"},
                    "out": {"graph": graph(g), "diagram": regia.piano_mermaid(d, g, command, "2026-01-10")}})
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dossier", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--random", type=int, default=120)
    parser.add_argument("--seed", type=int, default=444)
    a = parser.parse_args()
    tools = Path(a.dossier).resolve() / "tools"
    sys.path.insert(0, str(tools))
    sys.path.insert(0, str(tools / "tests"))
    regia = importlib.import_module("regia")
    words = Words()
    random_vectors = at_random(regia, words, a.random, a.seed)
    tested, runs = from_the_tests(regia, "test_piano_mermaid", words)
    tested.sort(key=lambda c: (c["fn"], json.dumps(c["args"], sort_keys=True, ensure_ascii=False)))
    vectors = {"defaults": DEFAULTS, "tests": runs, "vectors": tested + random_vectors}
    lines = ["{", f' "defaults": {json.dumps(DEFAULTS, ensure_ascii=False)},', f' "tests": {runs},', ' "vectors": [']
    body = [json.dumps(v, ensure_ascii=False, separators=(",", ":")) for v in vectors["vectors"]]
    lines += ["  " + item + ("," if i < len(body) - 1 else "") for i, item in enumerate(body)]
    lines += [" ]", "}"]
    text = "\n".join(lines) + "\n"
    assert json.loads(text) == json.loads(json.dumps(vectors, ensure_ascii=False))
    for real, _ in SYNTHETIC:
        assert real not in text, real
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(text, encoding="utf-8", newline="\n")
    print(f"{len(tested)} vectors from {runs} tests, {len(random_vectors)} random graphs")


if __name__ == "__main__":
    main()
