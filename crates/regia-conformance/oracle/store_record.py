"""Dev only: records the store's two fixtures from the prototype, `fixtures/store/iso_vectors.json` and
`fixtures/store/golden_rows.json`. CI replays them on the Rust store without Python (`tests/store_golden.rs`).

To record them again, from the workspace root, with the dossier checked out at the commit the fixtures name
(`oracle_commit`: a clone of the dossier fixed there, never the live checkout) and Python 3.12:

    python -I crates/regia-conformance/oracle/store_record.py --dossier <clone of the dossier at oracle_commit> \
        --commit <the full sha of that commit> --out fixtures/store

It loads `tools/runtime.py` from the dossier and changes nothing there. It sets the prototype's clock by
replacing `_adesso`, `_mono` and `_CLOCK` of the loaded module, and its local time to UTC by giving the module a
`datetime` whose `astimezone()` answers UTC, as `fixed_now.py` does for the whole prototype, so nothing in the
fixture depends on the machine. The state folder is a fresh one under `--scratch` (default: the `runtime`
folder of the dossier's clone, which git ignores), never the live one.

The file written carries the oracle: `oracle_commit`, `oracle_repository` and `python` (`sys.version`).

Two units are chosen here and the Rust test reads them back as they are: instants of the ISO vectors are
microseconds since the Unix epoch (the prototype's `datetime` resolution, so a fraction past the sixth digit is
told apart), and every number of a row is the JSON the prototype wrote, an integer or a float as it wrote it.
"""
import argparse
import datetime as _datetime
import importlib.util
import json
import os
import shutil
import sqlite3
import sys
from datetime import timedelta, timezone
from pathlib import Path

ORACLE_REPOSITORY = "lelexdrugo/regia"
# The tag both sides stamp the monotonic clock with: neither the system's nor the prototype's own.
CLOCK_TAG = "golden/clock"
# The prototype's names for the outcomes of a lock operation, in the words the Rust store uses.
STATES = {"preso": "taken", "rinnovato": "renewed", "rubato": "stolen", "rilasciato": "released",
          "libero": "free", "occupato": "busy", "altrui": "foreign", "assente": "absent"}
STATUSES = {"libero": "free", "occupato": "held", "scaduto": "expired", "illeggibile": "unreadable"}


def load_runtime(dossier):
    path = Path(dossier) / "tools" / "runtime.py"
    spec = importlib.util.spec_from_file_location("prototype_runtime", path)
    assert spec is not None and spec.loader is not None, f"cannot load {path}"
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# ---------- the ISO vectors ----------

def iso_vectors():
    """The strings `_parse` is asked about: the families the Rust port of `fromisoformat` was written from."""
    v = []
    # What the prototype writes, and the forms around it.
    v += ["2026-11-10T02:47:39.123+00:00", "2026-11-10T02:47:39.000+00:00", "2026-11-10T02:47:39.999+00:00",
          "2026-11-10T02:47:39+00:00", "2026-11-10T02:47:39Z", "2026-11-10T02:47:39", "2026-11-10T02:47:39.5",
          "2026-11-10T02:47", "2026-11-10T02", "2026-11-10", "1970-01-01T00:00:00+00:00",
          "0001-01-01T00:00:00+00:00", "9999-12-31T23:59:59.999999+00:00", "2024-02-29T12:00:00Z"]
    # Basic formats and mixed ones.
    v += ["20261110", "20261110T024739", "20261110T0247", "20261110T02", "20261110T024739Z", "20261110T024739.123Z",
          "2026-11-10T024739", "20261110T02:47:39", "2026-11-10T0247", "2026-11-10T02:4739", "2026-11-10T0247:39",
          "2026111", "202611", "2026-11", "2026-1-10", "2026-11-1", "26-11-10", "2026/11/10", "2026.11.10"]
    # ISO weeks.
    v += ["2026-W45", "2026-W45-2", "2026W45", "2026W452", "2026-W45-2T02:47:39", "2026W452T024739",
          "2026-W45T02:47", "2026W45T0247", "2026-W01-1", "2026-W52-7", "2026-W53-1", "2020-W53-7",
          "2015-W53-4", "2026-W00-1", "2026-W54-1", "2026-W45-0", "2026-W45-8", "2026-W4-1", "2026-W45-",
          "2026-W45-2-", "2026W45-2", "2026-W452", "2026-W", "2026-W4", "0001-W01-1", "9999-W52-5"]
    # Offsets: sign, shape, seconds, fractions of a second, size.
    v += ["2026-11-10T02:47:39+05:30", "2026-11-10T02:47:39-05:30", "2026-11-10T02:47:39+0530",
          "2026-11-10T02:47:39+05", "2026-11-10T02:47:39-05", "2026-11-10T02:47:39+05:30:15",
          "2026-11-10T02:47:39-05:30:15", "2026-11-10T02:47:39+05:30:15.5", "2026-11-10T02:47:39+05:30:15.123456",
          "2026-11-10T02:47:39+05:30:15.1234567", "2026-11-10T02:47:39+053015", "2026-11-10T02:47:39+00:00:00",
          "2026-11-10T02:47:39-00:00", "2026-11-10T02:47:39+00:00:00.5", "2026-11-10T02:47:39+23:59",
          "2026-11-10T02:47:39-23:59", "2026-11-10T02:47:39+24:00", "2026-11-10T02:47:39+23:59:59",
          "2026-11-10T02:47:39+23:59:59.999999", "2026-11-10T02:47:39+05:60", "2026-11-10T02:47:39+25:00",
          "2026-11-10T02:47:39+5", "2026-11-10T02:47:39+05:3", "2026-11-10T02:47:39+", "2026-11-10T02:47:39-",
          "2026-11-10T02:47:39+05:30Z", "2026-11-10T02:47:39+00:00Z", "2026-11-10T02:47:39ZZ",
          "2026-11-10T02:47:39+05:30 ", "0001-01-01T00:00:00+05:00", "9999-12-31T23:59:59-05:00",
          "2026-11-10TZ", "2026-11-10T02Z", "2026-11-10T02:47Z", "2026-11-10T02:47:39.5Z", "2026-11-10T02:47:39.5+01:00"]
    # Fractions of a second: lengths, separators, digits left over.
    v += ["2026-11-10T02:47:39.1", "2026-11-10T02:47:39.12", "2026-11-10T02:47:39.123", "2026-11-10T02:47:39.1234",
          "2026-11-10T02:47:39.12345", "2026-11-10T02:47:39.123456", "2026-11-10T02:47:39.1234567",
          "2026-11-10T02:47:39.123456789", "2026-11-10T02:47:39.9999999", "2026-11-10T02:47:39,5",
          "2026-11-10T02:47:39,123456", "2026-11-10T02:47:39.", "2026-11-10T02:47:39,", "2026-11-10T02:47:39.x",
          "2026-11-10T02:47:39.5x", "2026-11-10T02:47:39.5.5", "2026-11-10T02:47.5", "2026-11-10T02.5",
          "2026-11-10T02:47:39.1234567890123", "2026-11-10T02:47:39.1234567+00:00", "2026-11-10T024739.5",
          "2026-11-10T02:47:39.5 ", "2026-11-10T02:47:39:5"]
    # The time of day out of range, and shapes that are not one.
    v += ["2026-11-10T24:00:00", "2026-11-10T23:59:59", "2026-11-10T23:60:00", "2026-11-10T23:59:60",
          "2026-11-10T25:00:00", "2026-11-10T-1:00:00", "2026-11-10T2:47:39", "2026-11-10T02:7:39",
          "2026-11-10T02:47:3", "2026-11-10T02:47:39:", "2026-11-10T02-47-39", "2026-11-10T02;47;39"]
    # Separators between the date and the time: ASCII and not.
    v += ["2026-11-10 02:47:39", "2026-11-10t02:47:39", "2026-11-10_02:47:39", "2026-11-10é" "02:47:39",
          "2026-11-10 02:47:39", "2026-11-10　02:47:39", "2026-11-10€02:47:39", "2026-11-10\U0001f600" "02:47:39",
          "2026-11-10\u000002:47:39", "2026-11-10\u000b02:47:39", "20261110é02:47:39", "2026W452é02:47:39",
          "2026-11-10T02:47:39é", "2026-11-10T02:47:39 ", "2026-11-10T02:47:39+00:00é",
          "é2026-11-10", "2026-11-10é"]
    # Digits that are not ASCII, and a lowercase z.
    v += ["２０２６-11-10", "2026-11-10T０２:47:39", "٢٠٢٦-11-10",
          "2026-11-10T02:47:39z", "2026-11-10t02:47:39z", "2026-11-10T02:47:39 Z"]
    # Dates that do not exist, and those at the edge of the range.
    v += ["2026-02-29", "2024-02-29", "2100-02-29", "2000-02-29", "2026-04-31", "2026-06-31", "2026-13-01",
          "2026-00-10", "2026-11-00", "2026-11-31", "2026-12-31T23:59:59.999999", "0000-01-01", "0000-12-31",
          "10000-01-01", "-001-01-01", "+2026-11-10", "0001-01-01", "0001-01-01T00:00:00-00:01",
          "9999-12-31T23:59:59.999999-00:01", "9999-12-31T23:59:59+23:59", "0001-01-01T00:00:00+23:59"]
    # Empty, short, blank and not a date.
    v += ["", " ", "  ", "\t", "\n", "Z", "T", "now", "null", "None", "0", "1", "-1", "12345", "123456", "1234567",
          "12345678", "1794278859", "2026", "2026-", "2026-1", "T02:47:39", "02:47:39", " 2026-11-10T02:47:39",
          "2026-11-10T02:47:39 ", "2026-11-10 ", "2026-11-10\n", "èèèèèèè",
          "abcdefg", "2026-11-1X", "2026-11-10TT02:47:39", "2026-11-10T02:47:39+00:00+00:00"]
    seen, out = set(), []
    for text in v:
        if text not in seen:
            seen.add(text)
            out.append(text)
    return out


def record_iso(rt):
    epoch = rt._parse("1970-01-01T00:00:00+00:00")
    out = []
    for text in iso_vectors():
        try:
            parsed = rt._parse(text)
            if parsed is None:
                out.append({"text": text, "micros": None})
            else:
                delta = parsed - epoch
                out.append({"text": text, "micros": (delta.days * 86400 + delta.seconds) * 1_000_000 + delta.microseconds})
        except Exception as error:  # `_parse` catches ValueError and TypeError only: anything else is a crash
            out.append({"text": text, "micros": None, "raises": type(error).__name__})
    return out


# ---------- the sequence of rows ----------

START = "2026-01-10T09:00:00+00:00"
MONO_START = 1000.0


def window(used, reset_hours):
    """A budget window as `usage.py` writes one: local time to the minute, the distance in seconds."""
    return {"stato": "ok", "usato_pct": used, "residuo_pct": round(100 - used, 1),
            "reset": f"2026-01-10T{9 + reset_hours:02d}:00+00:00", "reset_tra_s": reset_hours * 3600}


def reading_ok(provider, used, reset_hours=2):
    return {"provider": provider, "stato": "ok", "piano": "max", "finestre": {"5h": window(used, reset_hours)}}


def reading_unknown(provider, reason):
    return {"provider": provider, "stato": "sconosciuto", "piano": None, "finestre": {}, "motivo": reason}


# What the probes answer, by name: the steps call them by name so the fixture holds them as data.
READINGS = {
    "both-ok": {"claude": reading_ok("claude", 40.0), "codex": reading_ok("codex", 12.5, 3)},
    "claude-429": {"claude": reading_unknown("claude", "too many requests (HTTP 429)"), "codex": reading_ok("codex", 13.0, 3)},
    "claude-recovers": {"claude": reading_ok("claude", 55.5, 1), "codex": reading_ok("codex", 14.0, 3)},
}

# The sequence: two projects, three holders, a steal after the expiry, a renewal by the other holder, a
# release, the status of each, and the budget cache and its probe claim. `advance` moves both clocks.
STEPS = [
    {"op": "status", "project": "alpha"},
    {"op": "acquire", "project": "alpha", "holder": "w1", "ttl": 600},
    {"op": "acquire", "project": "beta", "holder": "w2", "ttl": 60},
    {"op": "acquire", "project": "alpha", "holder": "w2", "ttl": 600},
    {"op": "status", "project": "alpha"},
    {"op": "advance", "seconds": 30},
    {"op": "acquire", "project": "alpha", "holder": "w1", "ttl": 900},
    {"op": "renew", "project": "alpha", "holder": "w1", "ttl": 1200},
    {"op": "renew", "project": "alpha", "holder": "w3", "ttl": 60},
    {"op": "renew", "project": "gamma", "holder": "w1", "ttl": 60},
    {"op": "release", "project": "alpha", "holder": "w3"},
    {"op": "advance", "seconds": 45},
    {"op": "status", "project": "beta"},
    {"op": "advance", "seconds": 20},
    {"op": "status", "project": "beta"},
    {"op": "acquire", "project": "beta", "holder": "w3", "ttl": 120},
    {"op": "advance", "seconds": 130},
    {"op": "status", "project": "beta"},
    {"op": "renew", "project": "beta", "holder": "w3", "ttl": 30},
    {"op": "acquire", "project": "beta", "holder": "w3", "ttl": 30.5},
    {"op": "release", "project": "beta", "holder": "w3"},
    {"op": "release", "project": "beta", "holder": "w3"},
    {"op": "acquire", "project": "beta", "holder": "w1", "ttl": 3600},
    {"op": "release", "project": "alpha", "holder": "w1"},
    {"op": "status", "project": "alpha"},
    {"op": "budget", "min_interval_s": 300, "force": False, "reading": "both-ok"},
    {"op": "advance", "seconds": 60},
    {"op": "budget", "min_interval_s": 300, "force": False, "reading": "claude-429"},
    {"op": "advance", "seconds": 300},
    {"op": "budget", "min_interval_s": 300, "force": False, "reading": "claude-429"},
    {"op": "advance", "seconds": 15},
    {"op": "budget", "min_interval_s": 300, "force": True, "reading": "claude-recovers"},
    {"op": "advance", "seconds": 5},
    {"op": "budget", "min_interval_s": 0, "force": False, "reading": "both-ok"},
]


class Clock:
    """The prototype's clock, which the steps move: the wall clock and the monotonic one, together."""

    def __init__(self, datetime_class):
        self.cls = datetime_class
        self.wall = datetime_class.fromisoformat(START)
        self.mono = MONO_START

    def advance(self, seconds):
        self.wall = self.wall + timedelta(seconds=seconds)
        self.mono = self.mono + float(seconds)


def pinned_datetime():
    """`datetime` whose local time is UTC: the prototype's `astimezone()` with no argument reads the machine's zone."""
    base = _datetime.datetime

    class Pinned(base):
        def astimezone(self, tz=None):
            aware = self if self.tzinfo is not None else self.replace(tzinfo=timezone.utc)
            return base.astimezone(aware, tz if tz is not None else timezone.utc)

    return Pinned


def rows_of(db):
    conn = sqlite3.connect(str(db))
    try:
        rows = conn.execute("SELECT tipo, chiave, gen, doc FROM documenti ORDER BY tipo, chiave").fetchall()
    finally:
        conn.close()
    return [{"kind": kind, "key": key, "gen": gen, "doc": json.loads(doc)} for kind, key, gen, doc in rows]


def normal(outcome):
    """The prototype's answer to a lock operation, in the words of the store: no prose."""
    held = outcome.get("attuale") or outcome
    out = {"ok": bool(outcome["ok"]), "state": STATES[outcome["stato"]]}
    out["holder"] = held.get("proprietario") if isinstance(held, dict) and "proprietario" in held else None
    out["expired"] = held.get("scaduto") if out["holder"] is not None else None
    out["expires_in_s"] = held.get("scade_tra_s") if out["holder"] is not None else None
    return out


def normal_status(status):
    out = {"busy": bool(status["occupato"]), "status": STATUSES[status["stato"]]}
    out["holder"] = status.get("proprietario")
    out["expired"] = status.get("scaduto") if out["holder"] is not None else None
    out["expires_in_s"] = status.get("scade_tra_s") if out["holder"] is not None else None
    return out


def record_rows(rt, scratch):
    folder = Path(scratch)
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)
    os.environ["REGIA_RUNTIME"] = str(folder)
    pinned = pinned_datetime()
    rt.datetime = pinned  # `_parse` reads this name: its instants have the pinned local time
    clock = Clock(pinned)
    rt._adesso = lambda: clock.wall
    rt._mono = lambda: clock.mono
    rt._CLOCK = CLOCK_TAG
    db = folder / rt.DB_NOME
    out = []
    for step in STEPS:
        record = dict(step)
        op = step["op"]
        if op == "advance":
            clock.advance(step["seconds"])
        elif op == "status":
            record["outcome"] = normal_status(rt.stato_lock(step["project"]))
        elif op == "budget":
            calls = []
            during = []

            def probe(reading=READINGS[step["reading"]]):
                calls.append(1)
                during.extend(rows_of(db))  # the claim of the probe is a row only while it runs
                return {"provider": json.loads(json.dumps(reading))}

            answer = rt.sonda_cache(step["min_interval_s"], sonda=probe, forza=step["force"])
            claude = answer["provider"]["claude"]
            record["outcome"] = {
                "probe_called": bool(calls), "from_cache": answer["cache"]["da_cache"], "generated": answer["generato"],
                "claude_status": {"ok": "ok", "sconosciuto": "unknown"}[claude["stato"]],"claude_used_pct": claude["finestre"].get("5h", {}).get("usato_pct"),
                "claude_age_s": claude.get("eta_s"), "claude_update_failed": "aggiornamento_fallito" in claude,
                "attempt_age_s": answer["cache"]["eta_tentativo_s"],
            }
            record["rows_during_probe"] = during if calls else None  # no probe, no claim
        else:
            fn = {"acquire": rt.acquisisci, "renew": rt.rinnova}.get(op)
            if fn:
                record["outcome"] = normal(fn(step["project"], step["holder"], step["ttl"]))
            else:
                record["outcome"] = normal(rt.rilascia(step["project"], step["holder"]))
        record["rows"] = rows_of(db)
        out.append(record)
    return {"clock": {"start": START, "monotonic_start": MONO_START, "tag": CLOCK_TAG},
            "readings": READINGS, "steps": out}


def header(args, rt):
    return {"oracle_commit": args.commit, "oracle_repository": ORACLE_REPOSITORY, "python": sys.version,
            "sqlite": rt.sqlite3.sqlite_version}


def main(argv):
    p = argparse.ArgumentParser(prog="store_record")
    p.add_argument("--dossier", required=True)
    p.add_argument("--commit", required=True, help="the full sha the dossier's clone is fixed at")
    p.add_argument("--out", required=True)
    p.add_argument("--scratch")
    args = p.parse_args(argv)
    if len(args.commit) != 40 or any(c not in "0123456789abcdef" for c in args.commit):
        print("store_record: --commit is the whole sha, 40 lowercase hexadecimal digits", file=sys.stderr)
        return 1
    if sys.version_info[:2] != (3, 12):
        print(f"store_record: Python 3.12 is the oracle's, this is {sys.version}", file=sys.stderr)
        return 1
    scratch = args.scratch or Path(args.dossier) / "runtime" / "store-record"
    rt = load_runtime(args.dossier)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    iso = {**header(args, rt), "unit": "microseconds since the Unix epoch, UTC", "vectors": record_iso(rt)}
    (out / "iso_vectors.json").write_text(json.dumps(iso, ensure_ascii=True, indent=1) + "\n", encoding="utf-8", newline="\n")
    rows = {**header(args, rt), **record_rows(rt, scratch)}
    (out / "golden_rows.json").write_text(json.dumps(rows, ensure_ascii=True, indent=1) + "\n", encoding="utf-8", newline="\n")
    shutil.rmtree(scratch, ignore_errors=True)
    print(f"{len(iso['vectors'])} vectors, {len(rows['steps'])} steps, Python {sys.version.split()[0]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
