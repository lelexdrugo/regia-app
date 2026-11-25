"""Dev only: the vectors of the checks (#448), captured from the prototype's own unit tests.

It runs the unit test classes of the dossier's `tools/tests/` that call `tools/controlli.py` with data only (no
git, no tracker), with every public function of that module wrapped, so that each outermost call is recorded:
its name and its arguments by parameter. The arguments are then made synthetic (no path, name or host of the
owner's machine) and written as the vectors, sorted and without repeats. `checks_oracle.py` replays them on the
prototype for the golden; the Rust side replays them on the port (`tests/checks_vectors.rs`).

    python -I checks_capture.py --dossier <dossier> --out ../tests/checks/vectors.json

The values that JSON has no word for are tagged: `{"$datetime": iso}`, `{"$set": [...]}`, `{"$tuple": [...]}`,
`{"$dict": [[key, value], ...]}` for a dict whose keys are not all strings, and a long string made of repeats
(the cap's tests write 30,000 characters) as `{"$runs": [[unit, count], ...]}`.
"""
import argparse
import datetime
import functools
import importlib
import inspect
import io
import json
import sys
import unittest
from pathlib import Path

sys.dont_write_bytecode = True

# The public functions of `tools/controlli.py` the port answers for. The text helpers of the prototype's own
# output (`riassunto`, `riga_voce`, `per_controllo`) are not here: the product prints its own, from the catalog.
FUNCTIONS = ("tronca", "riferimenti", "segmenti", "stato_senza_issue", "stato_righe_tolte", "domande_aperte", "ruolo_del_commento",
             "commenti_dopo_chiusura", "decisione_senza_flag", "inbox_ferme", "approvati_mai_partiti", "in_corso_fermi",
             "progetto_campo", "leggi_report_resta", "leggi_riconciliazioni", "classifica_destino", "resta_non_riconciliato",
             "percorso_assoluto", "decisioni_oltre_tetto", "controlla_progetto", "numeri_con_report")

# The unit test classes, by module: those of the wiring (a git repo, a mocked tracker) are the fixture cases' job.
CLASSES = {
    "test_controlli": ("TestSegmenti", "TestStatoSenzaIssue", "TestStatoRigheTolte", "TestDomandeAperte", "TestCommentiSulTracker",
                       "TestIssueFerme", "TestInsieme", "TestPercorsoAssoluto", "TestRestaTaskDelCampo", "TestDecisioniOltreTetto"),
    "test_resta_non_riconciliato": ("TestCasiDelContratto", "TestNessunaVoce", "TestLetturaDelleRighe"),
    "test_decisioni_tetto_crlf": ("TettoCrlfTest",),
    "test_campo_progetto": ("TestControllo",),
}
# Only these tests of a class that also reaches the wiring.
ONLY = {"TestControllo": ("test_campo_diverso_o_assente_e_una_voce", "test_issue_senza_etichetta_non_e_una_voce",
                          "test_si_attiva_solo_con_il_campo_configurato")}

# What the owner's machine and accounts write in the prototype's tests, and what the vectors say instead.
SYNTHETIC = (
    ("C:\\Users\\etern\\orca\\projects\\regia", "C:\\Users\\someone\\work\\dossier"),
    ("orca\\projects", "work"), ("orca/projects", "work"),
    ("etern", "someone"), ("lelexdrugo", "example"), ("regia-tasks", "tracker"), ("github.com", "tracker.invalid"),
    ("/home/gab/", "/home/user/"), ("/Users/gab/", "/Users/user/"), ("Experiment_Data", "data"),
)


def runs(text):
    """`text` as [[unit, count], ...], each unit repeated `count` times in a row; plain stretches have count 1."""
    out, i, plain = [], 0, []
    while i < len(text):
        best = (1, 1)
        for period in range(1, 65):
            unit, count = text[i:i + period], 1
            while text[i + count * period:i + (count + 1) * period] == unit:
                count += 1
            if count >= 3 and count * period > best[0] * best[1]:
                best = (period, count)
        if best[1] >= 3:
            if plain:
                out.append(["".join(plain), 1])
                plain = []
            out.append([text[i:i + best[0]], best[1]])
            i += best[0] * best[1]
        else:
            plain.append(text[i])
            i += 1
    if plain:
        out.append(["".join(plain), 1])
    assert "".join(u * n for u, n in out) == text
    return out


def encode(value):
    if isinstance(value, str) and len(value) > 1000:
        return {"$runs": runs(value)}
    if isinstance(value, datetime.datetime):
        return {"$datetime": value.isoformat()}
    if isinstance(value, (set, frozenset)):
        return {"$set": sorted((encode(v) for v in value), key=lambda v: json.dumps(v, sort_keys=True))}
    if isinstance(value, tuple):
        return {"$tuple": [encode(v) for v in value]}
    if isinstance(value, list):
        return [encode(v) for v in value]
    if isinstance(value, dict):
        if all(isinstance(k, str) for k in value):
            return {k: encode(v) for k, v in value.items()}
        return {"$dict": [[encode(k), encode(v)] for k, v in value.items()]}
    if isinstance(value, Path):
        return str(value)
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    raise TypeError(f"no encoding for {type(value).__name__}")


def synthetic(value):
    if isinstance(value, str):
        for real, fake in SYNTHETIC:
            value = value.replace(real, fake)
        return value
    if isinstance(value, list):
        return [synthetic(v) for v in value]
    if isinstance(value, dict):
        return {synthetic(k): synthetic(v) for k, v in value.items()}
    return value


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dossier", required=True)
    parser.add_argument("--out", required=True)
    a = parser.parse_args()
    tests = Path(a.dossier).resolve() / "tools" / "tests"
    sys.path.insert(0, str(tests.parent))
    sys.path.insert(0, str(tests))
    controlli = importlib.import_module("controlli")
    calls, depth = [], [0]

    def wrap(name, fn):
        signature = inspect.signature(fn)

        @functools.wraps(fn)
        def recorded(*args, **kwargs):
            outer = depth[0] == 0
            depth[0] += 1
            try:
                return fn(*args, **kwargs)
            finally:
                depth[0] -= 1
                if outer:
                    bound = signature.bind(*args, **kwargs)
                    bound.apply_defaults()
                    calls.append({"fn": name, "args": encode(dict(bound.arguments))})
        return recorded

    for name in FUNCTIONS:
        setattr(controlli, name, wrap(name, getattr(controlli, name)))
    suite = unittest.TestSuite()
    for module_name, classes in CLASSES.items():
        module = importlib.import_module(module_name)
        for class_name in classes:
            case = getattr(module, class_name)
            names = ONLY.get(class_name) or unittest.TestLoader().getTestCaseNames(case)
            suite.addTests(case(test) for test in names)
    result = unittest.TextTestRunner(stream=io.StringIO(), verbosity=0).run(suite)
    if not result.wasSuccessful():
        for test, trace in result.failures + result.errors:
            print(test, trace, file=sys.stderr)
        sys.exit("the prototype's tests did not pass: no vectors")
    seen, vectors = set(), []
    for call in calls:
        call = synthetic(call)
        key = json.dumps(call, sort_keys=True, ensure_ascii=False)
        if key not in seen:
            seen.add(key)
            vectors.append(call)
    vectors.sort(key=lambda c: (c["fn"], json.dumps(c["args"], sort_keys=True, ensure_ascii=False)))
    text = json.dumps(vectors, ensure_ascii=False, indent=1)
    for real, _ in SYNTHETIC:
        assert real not in text, real
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(text + "\n", encoding="utf-8", newline="\n")
    print(f"{len(vectors)} vectors from {result.testsRun} tests, {len(calls)} calls")


if __name__ == "__main__":
    main()
