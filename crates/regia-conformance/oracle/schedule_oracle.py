"""Dev only: the prototype's scheduled operations on a copy of the state, the Python side of
`tests/controller_schedule_cross.rs` (#469): the cursor `operazioni` the Rust controller wrote, read and used by the
prototype's own `_leggi_operazioni` and `_operazioni`. It loads the dossier's `tools/controller.py` and changes
nothing there. No chat opens: the launcher of chats is a fake that counts its calls. It refuses the live state folder.

    python -X utf8 -I -B schedule_oracle.py --dossier <dossier> --dir <folder> --config <file> --closings <folder>
        [--at <instant>]...

Without `--at` one tick at the system's clock; with it, one tick at each instant given (ISO, in order), on the
machine's local day as the prototype counts it (#732).

One JSON line on stdout: the voices of the ticks (name and action), each tick's operations in full (`records`), the
operations as `_leggi_operazioni` reads them after the last, the chats asked for, the first error, and the prototype's
limits of the days skipped (`limits`).
"""
import argparse
import json
import os
import sys
import time
from datetime import datetime
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
    parser.add_argument("--config", required=True)
    parser.add_argument("--closings", required=True)
    parser.add_argument("--at", action="append", default=[])
    a = parser.parse_args()
    instants = [datetime.fromisoformat(at.replace("Z", "+00:00")).timestamp() for at in a.at]
    folder = Path(a.dir).resolve()
    if any(folder == live.resolve() for live in live_folders()):
        print(json.dumps({"error": "the live state folder"}), flush=True)
        return 1
    os.environ["REGIA_RUNTIME"] = str(folder)
    sys.path.insert(0, str(Path(a.dossier) / "tools"))
    import controller as C  # noqa: E402
    import runtime  # noqa: E402

    calls = []

    def chat(nome, cartella, prompt, **kw):
        calls.append(nome)
        return {"adapter": "orca", "sessione": "python-chat", "log": None, "pid": None}

    config = json.loads(Path(a.config).read_text(encoding="utf-8"))
    ctl = C.Controller(C._parse(["--una-volta", "--lancio", "finto"]), runtime, cartelle=lambda: {}, config=config,
                       lancia_chat=chat, politica={"agente": "claude", "modello": "opus", "effort": "high", "fonte": "oracle"})
    ctl.dir_chiusure = Path(a.closings)
    ctl.prendi_lock()
    try:
        ticks = []
        for t0 in instants or [time.time()]:
            rec = {}
            ctl._operazioni(rec, t0)
            ticks.append(rec)
        ops = ctl._leggi_operazioni()
    finally:
        ctl.rilascia_lock()
    records = [rec.get("operazioni", []) for rec in ticks]
    voices = [[v["nome"], v["azione"]] for record in records for v in record]
    error = next((rec["operazioni_errore"] for rec in ticks if rec.get("operazioni_errore")), None)
    limits = {"lookback_days": C.SALTATE_LOOKBACK_GIORNI, "skipped_kept": C.SALTATE_TENUTE}
    print(json.dumps({"voices": voices, "records": records, "ops": ops, "calls": calls, "error": error,
                      "limits": limits}), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
