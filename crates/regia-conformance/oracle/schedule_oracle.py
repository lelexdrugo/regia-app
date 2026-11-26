"""Dev only: the prototype's scheduled operations on a copy of the state, the Python side of
`tests/controller_schedule_cross.rs` (#469): the cursor `operazioni` the Rust controller wrote, read and used by the
prototype's own `_leggi_operazioni` and `_operazioni`. It loads the dossier's `tools/controller.py` and changes
nothing there. No chat opens: the launcher of chats is a fake that counts its calls. It refuses the live state folder.

    python -X utf8 -I -B schedule_oracle.py --dossier <dossier> --dir <folder> --config <file> --closings <folder>

One JSON line on stdout: the voices of the tick, the operations as `_leggi_operazioni` reads them, the chats asked for.
"""
import argparse
import json
import os
import sys
import time
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
    a = parser.parse_args()
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
        rec = {}
        ctl._operazioni(rec, time.time())
        ops = ctl._leggi_operazioni()
    finally:
        ctl.rilascia_lock()
    voices = [[v["nome"], v["azione"]] for v in rec.get("operazioni", [])]
    print(json.dumps({"voices": voices, "ops": ops, "calls": calls, "error": rec.get("operazioni_errore")}), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
