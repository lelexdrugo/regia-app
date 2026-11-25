"""Dev only: the prototype's answers to the vectors of the checks (#448), for the golden the port is held to.

It loads `tools/controlli.py` from the dossier, as `tools/regia.py` loads it, and changes nothing there. Each
vector of `tests/checks/vectors.json` (captured from the prototype's unit tests by `checks_capture.py`) and of
`tests/checks/unicode.json` (written by hand: Python's rules for text where the `regex` crate's differ) is a
call, `{"fn": name, "args": {parameter: value}}`, in the tags of `checks_capture.py`; its answer is written in
the same tags, in the same order. `carica_soglie` takes the text of the file, which is written under `--dir`.

    python -I checks_oracle.py --dossier <dossier> --dir <folder under target/> --out ../tests/checks/python.json
"""
import argparse
import datetime
import importlib.util
import json
import subprocess
import sys
from pathlib import Path

sys.dont_write_bytecode = True
HERE = Path(__file__).resolve().parent
VECTORS = HERE.parent / "tests" / "checks"


def decode(value):
    if isinstance(value, list):
        return [decode(v) for v in value]
    if isinstance(value, dict):
        if set(value) == {"$datetime"}:
            return datetime.datetime.fromisoformat(value["$datetime"])
        if set(value) == {"$set"}:
            return {decode(v) for v in value["$set"]}
        if set(value) == {"$tuple"}:
            return tuple(decode(v) for v in value["$tuple"])
        if set(value) == {"$dict"}:
            return {decode(k): decode(v) for k, v in value["$dict"]}
        if set(value) == {"$runs"}:
            return "".join(unit * count for unit, count in value["$runs"])
        return {k: decode(v) for k, v in value.items()}
    return value


def encode(value):
    if isinstance(value, datetime.datetime):
        return {"$datetime": value.isoformat()}
    if isinstance(value, datetime.date):
        return {"$date": value.isoformat()}
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
    if value is None or isinstance(value, (bool, int, float, str)):
        return value
    raise TypeError(f"no encoding for {type(value).__name__}")


def load(dossier):
    spec = importlib.util.spec_from_file_location("regia_controlli", dossier / "tools" / "controlli.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dossier", required=True)
    parser.add_argument("--dir", required=True)
    parser.add_argument("--out", required=True)
    a = parser.parse_args()
    dossier = Path(a.dossier).resolve()
    work = Path(a.dir).resolve()
    work.mkdir(parents=True, exist_ok=True)
    controlli = load(dossier)
    results = []
    for name in ("vectors.json", "unicode.json"):
        for i, vector in enumerate(json.loads((VECTORS / name).read_text(encoding="utf-8"))):
            args = decode(vector["args"])
            if vector["fn"] == "carica_soglie":
                path = work / f"{Path(name).stem}-{i}.yaml"
                path.write_bytes(args["testo"].encode("utf-8"))
                result = controlli.carica_soglie(path)
            elif vector["fn"] == "data_roma":
                result = controlli._data_roma(args["s"])
            else:
                result = getattr(controlli, vector["fn"])(**args)
            results.append({"file": name, "index": i, "fn": vector["fn"], "result": encode(result)})
    commit = subprocess.run(["git", "-C", str(dossier), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    text = json.dumps({"oracle_commit": commit, "python": sys.version.split()[0], "results": results}, ensure_ascii=False, indent=1)
    Path(a.out).write_text(text + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
