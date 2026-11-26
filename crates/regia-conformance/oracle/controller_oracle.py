"""Dev only: the prototype's resident controller holding its lock on a copy of the state, the Python side of the
cross proofs of #468 (`tests/controller_cross.rs`). It runs the dossier's `tools/controller.py` through its own
`main`, with the hooks the prototype's tests give it: a precheck that answers idle (no tracker, no `gh`), the fake
launcher, no resume, no wake-up, no scheduled operation. It changes nothing in the dossier, and refuses the live
state folder, where the real controller holds the real lock.

    python -X utf8 -I -B controller_oracle.py --dossier <dossier> --dir <folder>

Once the lock is taken and the first tick ran, it prints one JSON line `{"ready": <the lock's holder>}` and waits
in the controller's sleep until its stdin closes; then it stops as Ctrl+C stops the controller, which releases the
lock, and exits with the controller's code.
"""
import argparse
import io
import json
import os
import sys
from pathlib import Path


def live_folders():
    """The folders a real installation uses, whatever REGIA_RUNTIME says."""
    out = []
    for var, parts in (("LOCALAPPDATA", ()), ("XDG_STATE_HOME", ()), ("USERPROFILE", ("AppData", "Local")),
                       ("HOME", ("AppData", "Local")), ("HOME", (".local", "state")),
                       ("USERPROFILE", (".local", "state"))):
        value = os.environ.get(var)
        if value:
            out.append(Path(value, *parts, "regia"))
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dossier", required=True)
    parser.add_argument("--dir", required=True)
    a = parser.parse_args()
    folder = Path(a.dir).resolve()
    if any(folder == live.resolve() for live in live_folders()):
        print(json.dumps({"error": "the live state folder"}), flush=True)
        return 1
    os.environ["REGIA_RUNTIME"] = str(folder)
    sys.path.insert(0, str(Path(a.dossier) / "tools"))
    import controller as C  # noqa: E402
    import runtime  # noqa: E402

    def precheck(projects, timeout_s):
        line = {"v": 1, "outcome": "idle", "exit": 3, "motivo": "nessun-evento", "cursore": [], "payload": {"progetti": []},
                "t": "2026-01-10T09:00:00Z"}
        return {"exit": 3, "json": line, "errore": None}

    def sleep(seconds):
        print(json.dumps({"ready": runtime.stato_lock(C.LOCK_NOME).get("proprietario")}), flush=True)
        sys.stdin.read()
        raise KeyboardInterrupt

    argv = ["--giri", "2", "--intervallo", "3600", "--lancio", "finto", "--no-ripresa", "--no-sveglia", "--progetto", "p468",
            "--config", str(folder / "no-operations.json")]
    policy = {"agente": "claude", "modello": "opus", "effort": "high", "fonte": "test"}
    return C.main(argv, precheck=precheck, dormi=sleep, cartelle=lambda: {}, config={"operazioni": []}, politica=policy,
                  out=io.StringIO(), err=io.StringIO())


if __name__ == "__main__":
    sys.exit(main())
