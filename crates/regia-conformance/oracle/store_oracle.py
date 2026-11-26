"""Dev only: the prototype's runtime as one process on a state database, the Python side of the
store's cross tests. It loads `tools/runtime.py` from the dossier and changes nothing there. It takes the
folder only from --dir and refuses the live state folder, where a real controller holds the real lock.

    python store_oracle.py --dossier <dossier> --dir <folder> [--wall-offset-s S] [--busy-wait S] <command> ...

Commands, as store-probe: `acquire <project> <holder> <ttl>`, `renew <project> <holder> <ttl>`,
`release <project> <holder>` (each takes `--start-at-ms <epoch ms>`: wait for that instant, so that several
processes make the call together), `status <project>`, `hold-tx`, `contend <project> <holder> <attempts>
--witness <folder> --start-at-ms <ms> --hold-ms <ms> --done <file> --peer-done <file>`. Also
`plant <kind> <key> <text>` (a raw row, as the prototype's tests write one) and `parse <vectors.json>`.

One JSON line on stdout. Exit 0 when done, 3 when refused, as the prototype's CLI, 1 on an error or a
broken invariant.
"""
import argparse
import importlib.util
import json
import os
import random
import sys
import time
from datetime import timedelta
from pathlib import Path

# The prototype's statement for its one table, as its tests write rows by hand.
PLANT = ("INSERT INTO documenti (tipo, chiave, gen, doc) VALUES (?, ?, 1, ?) "
         "ON CONFLICT (tipo, chiave) DO UPDATE SET gen = gen + 1, doc = excluded.doc")


def load_runtime(dossier):
    path = Path(dossier) / "tools" / "runtime.py"
    spec = importlib.util.spec_from_file_location("prototype_runtime", path)
    assert spec is not None and spec.loader is not None, f"cannot load {path}"
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


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


def ttl(text):
    try:
        return int(text)
    except ValueError:
        return float(text)


def emit(value):
    print(json.dumps(value), flush=True)


def contend(rt, a):
    witness = Path(a.witness) / "inside"
    while time.time() * 1000 < a.start_at_ms:
        pass
    made = wins = busy = contended = violations = 0
    anomalies = []
    started = time.monotonic()
    while True:
        made += 1
        e = rt.acquisisci(a.project, a.holder, 60)
        if e.get("ok") and e.get("stato") in ("preso", "rubato"):
            wins += 1
            try:
                witness.mkdir()
            except FileExistsError:
                violations += 1
            except OSError as error:
                anomalies.append(f"witness: {error}")
            time.sleep(random.random() * a.hold_ms / 1000)
            try:
                witness.rmdir()
            except FileNotFoundError:
                violations += 1
            except OSError as error:
                anomalies.append(f"witness: {error}")
            r = rt.rilascia(a.project, a.holder)
            if r.get("stato") != "rilasciato":
                anomalies.append(f"release at attempt {made}: {r}")
        elif not e.get("ok") and e.get("stato") == "occupato":
            # The prototype tells the two only in its prose: "in contesa, riprova" is the busy database.
            if str(e.get("motivo", "")).endswith("riprova"):
                contended += 1
            else:
                busy += 1
        else:
            anomalies.append(f"acquire at attempt {made}: {e}")
        if made == a.attempts:
            Path(a.done).write_bytes(b"")
        if made >= a.attempts and Path(a.peer_done).exists():
            break
        time.sleep(random.random() / 1000)
    emit({"side": "python", "python": sys.version.split()[0], "sqlite": rt.sqlite3.sqlite_version,
          "clock": rt._CLOCK, "attempts": made, "wins": wins, "busy": busy,
          "contended": contended, "violations": violations, "anomalies": anomalies[:20],
          "seconds": time.monotonic() - started})
    return 0 if violations == 0 and not anomalies else 1


def main(argv):
    p = argparse.ArgumentParser(prog="store_oracle")
    p.add_argument("--dossier", required=True)
    p.add_argument("--dir", required=True)
    p.add_argument("--wall-offset-s", type=float)
    p.add_argument("--busy-wait", type=float)
    sub = p.add_subparsers(dest="command", required=True)
    for name in ("acquire", "renew"):
        s = sub.add_parser(name)
        s.add_argument("project")
        s.add_argument("holder")
        s.add_argument("ttl", type=ttl)
        s.add_argument("--start-at-ms", type=float)
    s = sub.add_parser("release")
    s.add_argument("project")
    s.add_argument("holder")
    s.add_argument("--start-at-ms", type=float)
    s = sub.add_parser("status")
    s.add_argument("project")
    sub.add_parser("hold-tx")
    s = sub.add_parser("contend")
    s.add_argument("project")
    s.add_argument("holder")
    s.add_argument("attempts", type=int)
    s.add_argument("--witness", required=True)
    s.add_argument("--start-at-ms", type=float, required=True)
    s.add_argument("--hold-ms", type=float, default=2.0)
    s.add_argument("--done", required=True)
    s.add_argument("--peer-done", required=True)
    s = sub.add_parser("plant")
    s.add_argument("kind")
    s.add_argument("key")
    s.add_argument("text")
    s = sub.add_parser("parse")
    s.add_argument("vectors")
    a = p.parse_args(argv)

    folder = Path(a.dir).resolve()
    for live in live_folders():
        if folder == live.resolve() or live.resolve() in folder.parents:
            print(f"store_oracle: {folder} is inside the live state folder {live}: refused", file=sys.stderr)
            return 1
    os.environ["REGIA_RUNTIME"] = str(folder)
    rt = load_runtime(a.dossier)
    if a.wall_offset_s is not None:
        real = rt._adesso
        rt._adesso = lambda: real() + timedelta(seconds=a.wall_offset_s)
    if a.busy_wait is not None:
        rt.ATTESA_S = a.busy_wait

    if a.command in ("acquire", "renew", "release") and a.start_at_ms is not None:
        while time.time() * 1000 < a.start_at_ms:  # the barrier of the eight processes: all start the call together
            pass
    if a.command == "acquire":
        e = rt.acquisisci(a.project, a.holder, a.ttl)
    elif a.command == "renew":
        e = rt.rinnova(a.project, a.holder, a.ttl)
    elif a.command == "release":
        e = rt.rilascia(a.project, a.holder)
    elif a.command == "status":
        emit(rt.stato_lock(a.project))
        return 0
    elif a.command == "hold-tx":
        conn = rt._apri()
        conn.execute("BEGIN IMMEDIATE")
        emit("ready")
        sys.stdin.read()
        conn.execute("ROLLBACK")
        conn.close()
        return 0
    elif a.command == "contend":
        return contend(rt, a)
    elif a.command == "plant":
        conn = rt._apri()
        try:
            conn.execute(PLANT, (a.kind, a.key, a.text))
        finally:
            conn.close()
        emit("planted")
        return 0
    else:  # parse
        vectors = json.loads(Path(a.vectors).read_text(encoding="utf-8"))
        out = []
        for text in vectors:
            parsed = rt._parse(text)
            if parsed is None:
                out.append([text, None])
            else:
                delta = parsed - rt._parse("1970-01-01T00:00:00+00:00")
                out.append([text, (delta.days * 86400 + delta.seconds) * 1_000_000 + delta.microseconds])
        emit({"python": sys.version.split()[0], "vectors": out})
        return 0
    emit(e)
    return 0 if e.get("ok") else 3


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
