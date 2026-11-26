"""Dev only: records the store's three fixtures from the prototype, `fixtures/store/iso_vectors.json`,
`fixtures/store/golden_rows.json` (#561: the lock, the budget cache and its probe) and
`fixtures/store/golden_families.json` (#730: a task's lease, the cursors and the orchestrator's step). CI
replays them on the Rust store without Python (`tests/store_golden.rs`).

To record them again, from the workspace root, with the dossier checked out at the commit the fixtures name
(`oracle_commit`: a clone of the dossier fixed there, never the live checkout) and Python 3.12:

    python -I crates/regia-conformance/oracle/store_record.py --dossier <clone of the dossier at oracle_commit> \
        --commit <the full sha of that commit> --out fixtures/store

Each file names the commit it was recorded at, and one run writes all three at the commit it is given: a file
of an older commit whose rows come out the same but for its header may stay as it is (#730 left the two files
of #561 at theirs, after a run at its own commit gave the same rows).

It loads `tools/runtime.py` from the dossier and, for the step, `tools/regia.py`, and changes nothing there. It
sets the prototype's clock by replacing `_adesso`, `_mono` and `_CLOCK` of the loaded module, and its local time
to UTC by giving the module a `datetime` whose `astimezone()` answers UTC, as `fixed_now.py` does for the whole
prototype, so nothing in the fixture depends on the machine. The step's commands run through the prototype's own
`main()` in this process, with `--no-push`, on the same clock: their instant is `datetime.now` of `regia.py`,
which reads it, and `runtime.py` is the module whose clock is set. The session the step and a comment read is
the one each step names, never this process's. The state folder is a fresh one under `--scratch` (default: the
`runtime` folder of the dossier's clone, which git ignores), never the live one.

The files written carry the oracle: `oracle_commit`, `oracle_repository` and `python` (`sys.version`). The
fixtures are public: before writing, the families' file is checked for any path, user, machine or session of
the run.

Two units are chosen here and the Rust test reads them back as they are: instants of the ISO vectors are
microseconds since the Unix epoch (the prototype's `datetime` resolution, so a fraction past the sixth digit is
told apart), and every number of a row is the JSON the prototype wrote, an integer or a float as it wrote it.
"""
import argparse
import contextlib
import datetime as _datetime
import importlib.util
import io
import json
import os
import re
import shutil
import socket
import sqlite3
import sys
from datetime import timedelta, timezone
from pathlib import Path

ORACLE_REPOSITORY = "lelexdrugo/regia"
# The tag both sides stamp the monotonic clock with: neither the system's nor the prototype's own.
CLOCK_TAG = "golden/clock"
# The prototype's names for the outcomes of a lock operation, in the words the Rust store uses.
STATES = {"preso": "taken", "rinnovato": "renewed", "rubato": "stolen", "rilasciato": "released",
          "libero": "free", "occupato": "busy", "altrui": "foreign", "assente": "absent", "battito": "heartbeat"}
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


# ---------- the families of #730: a task's lease, the cursors, the orchestrator's step ----------

FAMILIES = "golden_families.json"
# The variables a session's id is read from: none of this process's may reach the steps or the file.
SESSION_VARIABLES = ("CLAUDE_CODE_SESSION_ID", "ORCA_AGENT_SESSION_ID", "CODEX_THREAD_ID")
# The one the step and a comment read (`_sessione` of `regia.py`).
STEP_SESSION = "CLAUDE_CODE_SESSION_ID"

# Three tasks whose keys sort by length first ("7", "12", "100"), three holders, two projects: a take, a
# refusal, an expiry to the second, heartbeats with and without a new ttl and by another holder, a renewal
# that moves the lease, takes of an expired lease by its holder and by another, releases, and the list.
LEASE_STEPS = [
    {"op": "lease-list", "expired": False},
    {"op": "lease-take", "task": 7, "project": "alpha", "holder": "w1", "ttl": 600},
    {"op": "lease-take", "task": 12, "project": "beta", "holder": "w2", "ttl": 60},
    {"op": "lease-take", "task": 100, "project": "alpha", "holder": "w3", "ttl": 30},
    {"op": "lease-take", "task": 7, "project": "alpha", "holder": "w2", "ttl": 600},
    {"op": "lease-list", "expired": False},
    {"op": "lease-list", "expired": True},
    {"op": "advance", "seconds": 30},
    {"op": "lease-list", "expired": True},
    {"op": "lease-beat", "task": 7, "holder": "w1", "ttl": None},
    {"op": "lease-beat", "task": 7, "holder": "w1", "ttl": 900},
    {"op": "lease-beat", "task": 7, "holder": "w2", "ttl": None},
    {"op": "lease-beat", "task": 99, "holder": "w1", "ttl": None},
    {"op": "lease-take", "task": 7, "project": "beta", "holder": "w1", "ttl": 300.5},
    {"op": "lease-beat", "task": 100, "holder": "w3", "ttl": None},
    {"op": "advance", "seconds": 45},
    {"op": "lease-list", "expired": False},
    {"op": "lease-list", "expired": True},
    {"op": "lease-take", "task": 12, "project": "alpha", "holder": "w3", "ttl": 120},
    {"op": "lease-beat", "task": 12, "holder": "w2", "ttl": None},
    {"op": "lease-release", "task": 12, "holder": "w2"},
    {"op": "lease-take", "task": 100, "project": "alpha", "holder": "w3", "ttl": 45},
    {"op": "lease-beat", "task": 100, "holder": "w3", "ttl": 30.0015},
    {"op": "lease-release", "task": 12, "holder": "w3"},
    {"op": "lease-release", "task": 12, "holder": "w3"},
    {"op": "lease-list", "expired": False},
    {"op": "advance", "seconds": 600},
    {"op": "lease-take", "task": 7, "project": "alpha", "holder": "w2", "ttl": 60},
    {"op": "lease-release", "task": 7, "holder": "w2"},
    {"op": "lease-release", "task": 100, "holder": "w1"},
    {"op": "lease-list", "expired": True},
    {"op": "lease-list", "expired": False},
]

# The events cursor (no name: the project's id is the key) and a named one, absent with and without a default,
# an instant (the old form of the events cursor), a list, the parts a step's end writes, numbers of each kind,
# the events cursor by its name, a null value, a name the prototype refuses, a project with no cursor.
CURSOR_STEPS = [
    {"op": "cursor-read", "project": "alpha"},
    {"op": "cursor-read", "project": "alpha", "default": "none yet"},
    {"op": "cursor-read", "project": "alpha", "name": "notes", "default": []},
    {"op": "cursor-write", "project": "alpha", "value": "2026-01-09T18:00:00Z"},
    {"op": "cursor-read", "project": "alpha"},
    {"op": "cursor-write", "project": "alpha", "name": "notes", "value": [3, 1, 2]},
    {"op": "cursor-read", "project": "alpha", "name": "notes"},
    {"op": "cursor-read", "project": "alpha"},
    {"op": "advance", "seconds": 0.25},
    {"op": "cursor-write", "project": "alpha", "value": {"v": 1, "t": "2026-01-10T09:00:00Z", "propri": {"commenti": [5, 9]}}},
    {"op": "cursor-read", "project": "alpha"},
    {"op": "cursor-write", "project": "beta", "value": {"n": 1, "x": 1.0, "big": 9007199254740993, "neg": -2, "half": 0.5,
                                                         "text": "è", "nested": {"list": [True, None, 2.0]}}},
    {"op": "cursor-read", "project": "beta"},
    {"op": "advance", "seconds": 1.75},
    {"op": "cursor-write", "project": "alpha", "name": "eventi", "value": "2026-01-10T09:30:00Z"},
    {"op": "cursor-read", "project": "alpha"},
    {"op": "cursor-read", "project": "alpha", "name": "eventi"},
    {"op": "cursor-write", "project": "alpha", "name": "notes", "value": None},
    {"op": "cursor-read", "project": "alpha", "name": "notes"},
    {"op": "cursor-write", "project": "alpha", "name": "Notes", "value": 1},
    {"op": "cursor-read", "project": "gamma", "name": "notes"},
]

# The step's commands as an orchestrator runs them, without `ttl` the prototype's default (an integer), with one
# a float (`--ttl` is a float): a step opened, its comments recorded (twice the same, another session's, one with
# no session), renewed, refused to another holder, resumed by its holder in another session, closed with a
# report it processed and one that is missing, closed again; the events cursor in the old form read by a step;
# a lock taken without a step, whose finish is an error; steals of an expired lock with the step of another left
# open; a step resumed after its own lock expired; a finish on an expired lock nobody took.
STEP_STEPS = [
    {"op": "step-start", "project": "alpha", "holder": "o1", "session": "s-1"},
    {"op": "step-comment", "project": "alpha", "comment": 101, "session": "s-1"},
    {"op": "step-comment", "project": "alpha", "comment": 55, "session": "s-1"},
    {"op": "step-comment", "project": "alpha", "comment": 101, "session": "s-1"},
    {"op": "step-comment", "project": "alpha", "comment": 77, "session": "s-2"},
    {"op": "step-comment", "project": "alpha", "comment": 78, "session": None},
    {"op": "advance", "seconds": 600.75},
    {"op": "step-renew", "project": "alpha", "holder": "o1", "session": "s-1"},
    {"op": "step-renew", "project": "alpha", "holder": "o2", "session": "s-2", "ttl": 600.0},
    {"op": "step-start", "project": "alpha", "holder": "o2", "session": "s-2"},
    {"op": "step-start", "project": "alpha", "holder": "o1", "session": "s-2", "ttl": 900.0},
    {"op": "step-comment", "project": "alpha", "comment": 79, "session": "s-1"},
    {"op": "step-finish", "project": "alpha", "holder": "o2", "session": "s-2"},
    {"op": "step-finish", "project": "alpha", "holder": "o1", "session": "s-2",
     "reports": {"2026-01-09-3-done.md": "2026-01-10T08:00:00+00:00", "missing.md": None}},
    {"op": "step-finish", "project": "alpha", "holder": "o1", "session": "s-2"},
    {"op": "step-comment", "project": "alpha", "comment": 80, "session": "s-1"},
    {"op": "cursor-write", "project": "beta", "value": "2026-01-09T18:00:00Z"},
    {"op": "advance", "seconds": 3.5},
    {"op": "step-start", "project": "beta", "holder": "o1", "session": None},
    {"op": "acquire", "project": "alpha", "holder": "o2", "ttl": 60},
    {"op": "step-finish", "project": "alpha", "holder": "o2", "session": None},
    {"op": "advance", "seconds": 61},
    {"op": "step-start", "project": "alpha", "holder": "o1", "session": "s-1"},
    {"op": "step-finish", "project": "alpha", "holder": "o1", "session": "s-1"},
    {"op": "step-start", "project": "gamma", "holder": "o2", "session": "s-2", "ttl": 60.0},
    {"op": "step-comment", "project": "gamma", "comment": 300, "session": "s-2"},
    {"op": "advance", "seconds": 61},
    {"op": "step-start", "project": "gamma", "holder": "o1", "session": "s-1"},
    {"op": "step-finish", "project": "gamma", "holder": "o1", "session": "s-1"},
    {"op": "step-start", "project": "delta", "holder": "o1", "session": "s-1", "ttl": 30.0},
    {"op": "advance", "seconds": 31},
    {"op": "step-start", "project": "delta", "holder": "o1", "session": "s-1"},
    {"op": "advance", "seconds": 1800},
    {"op": "step-finish", "project": "beta", "holder": "o1", "session": None},
    {"op": "step-finish", "project": "delta", "holder": "o1", "session": "s-1"},
]

# The prototype's errors of a step, by the start of their sentence, in the words of the store.
STEP_ERRORS = {"errore: nessun passo aperto": "no-step"}
# The prototype's refusal of a cursor's name, by the start of its sentence.
CURSOR_NAME_REFUSED = "nome di cursore non valido"


def load_cli(dossier, rt):
    """`tools/regia.py`, with `runtime.py` the module whose clock is set and no line in the sessions' log."""
    path = Path(dossier) / "tools" / "regia.py"
    spec = importlib.util.spec_from_file_location("prototype_cli", path)
    assert spec is not None and spec.loader is not None, f"cannot load {path}"
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    setattr(module, "_runtime", lambda: rt)  # it loads `runtime.py` afresh at every call, on the real clock
    setattr(module, "_registra_attivita", lambda _: None)  # the sessions' log is not a row of the store
    return module


def clocked_datetime(pinned, clock):
    """`pinned` whose `now` reads `clock`: the step takes its instant from `datetime.now` of `regia.py`."""

    class Clocked(pinned):
        @classmethod
        def now(cls, tz=None):
            return clock.wall.astimezone(tz) if tz is not None else clock.wall.replace(tzinfo=None)

    return Clocked


@contextlib.contextmanager
def session(value):
    """The session id of the step, `None` for none; none before and after it."""
    if value is None:
        os.environ.pop(STEP_SESSION, None)
    else:
        os.environ[STEP_SESSION] = value
    try:
        yield
    finally:
        os.environ.pop(STEP_SESSION, None)


def events_form(value):
    """The events cursor as the store reads it (`EventsCursor`): absent, the old form (the instant alone), its
    parts, or a value of another kind."""
    if value is None:
        return {"form": "absent"}
    if isinstance(value, str):
        return {"form": "instant", "instant": value}
    if isinstance(value, dict):
        own = value.get("propri")
        parts = own if isinstance(own, dict) else {}
        return {"form": "parts", "version": value.get("v"), "t": value.get("t"),
                "own": {"comments": parts.get("commenti"), "reports": parts.get("report")} if "propri" in value else None}
    return {"form": "other", "value": value}


def normal_entry(view):
    """A lease of `elenca`, in the words of the store: the fields of its document as the view shows them."""
    if view.get("illeggibile"):
        return {"unreadable": True, "task": view["task"], "expired": True, "expires_in_s": 0, "fields": None}
    fields = {key: value for key, value in view.items() if key not in ("scaduto", "scade_tra_s")}
    return {"unreadable": False, "expired": view["scaduto"], "expires_in_s": view["scade_tra_s"], "fields": fields}


def normal_step(op, code, stdout, stderr):
    """The answer of a step's command in the words of the store, with its exit code; no prose."""
    if code == 1:
        known = [word for start, word in STEP_ERRORS.items() if stderr.startswith(start)]
        assert len(known) == 1, f"an error of the step the table does not know: {stderr!r}"
        return {"exit": 1, "error": known[0]}
    answer = json.loads(stdout)
    if code == 3 or op == "step-renew":
        return {"exit": code, **normal(answer)}
    assert code == 0, f"{op}: exit {code}"
    if op == "step-start":
        return {"exit": 0, "ok": True, "state": STATES[answer["stato"]], "holder": answer["proprietario"],
                "expires_in_s": answer["scade_tra_s"], "t": answer["t"], "resumed": answer["ripreso"],
                "cursor": events_form(answer["cursore"])}
    return {"exit": 0, "ok": True, "holder": answer["proprietario"], "cursor": events_form(answer["cursore"]),
            "lock": STATES[answer["lock"]]}


def step_command(cli, step, folder):
    """`regia passo inizio | rinnova | fine` through the prototype's own command line, in this process."""
    sub = {"step-start": "inizio", "step-renew": "rinnova", "step-finish": "fine"}[step["op"]]
    argv = ["--json", "passo", sub, "--progetto", step["project"], "--proprietario", step["holder"]]
    if "ttl" in step:
        assert isinstance(step["ttl"], float), "the command line's --ttl is a float: the fixture holds it as one"
        argv += ["--ttl", repr(step["ttl"])]
    if sub == "fine":
        argv.append("--no-push")
        reports = step.get("reports") or {}
        if reports:
            # The reports the step processed: those with a date exist, with that date as their time.
            dossier = folder / "dossier" / step["project"]
            (dossier / ".regia" / "report").mkdir(parents=True, exist_ok=True)
            for name, date in reports.items():
                if date is not None:
                    path = dossier / ".regia" / "report" / name
                    path.write_text("synthetic\n", encoding="utf-8")
                    moment = _datetime.datetime.fromisoformat(date).timestamp()
                    os.utime(path, (moment, moment))
            argv += ["--cartella", str(dossier), "--report-elaborati", *reports]
    stdout, stderr = io.StringIO(), io.StringIO()
    code, before = 0, sys.argv
    sys.argv = ["regia", *argv]
    try:
        with session(step["session"]), contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
            cli.main()
    except SystemExit as error:
        code = error.code if isinstance(error.code, int) else 1
    finally:
        sys.argv = before
    return normal_step(step["op"], code, stdout.getvalue(), stderr.getvalue())


def family_op(rt, cli, step, folder):
    """One step of a family on the prototype, and its outcome in the words of the store."""
    op = step["op"]
    copy = lambda value: json.loads(json.dumps(value))  # noqa: E731 - the prototype never holds the step's own data
    if op == "acquire":
        return normal(rt.acquisisci(step["project"], step["holder"], step["ttl"]))
    if op == "lease-take":
        return normal(rt.prendi(step["task"], step["project"], step["holder"], step["ttl"]))
    if op == "lease-beat":
        return normal(rt.battito(step["task"], step["holder"], step["ttl"]))
    if op == "lease-release":
        return normal(rt.rilascia_lease(step["task"], step["holder"]))
    if op == "lease-list":
        return [normal_entry(view) for view in rt.elenca(scadute=step["expired"])]
    if op == "cursor-read":
        name = step.get("name", rt.NOME_CURSORE)
        if "default" in step:
            return {"value": rt.leggi_cursore(step["project"], copy(step["default"]), nome=name)}
        return {"value": rt.leggi_cursore(step["project"], nome=name)}
    if op == "cursor-write":
        try:
            rt.scrivi_cursore(step["project"], copy(step["value"]), nome=step.get("name", rt.NOME_CURSORE))
        except rt.ErroreRuntime as error:
            assert str(error).startswith(CURSOR_NAME_REFUSED), f"not a refused name: {error}"
            return {"error": "invalid-name"}
        return None
    if op == "step-comment":
        # The half of `regia commento` that is the store's: the tracker's comment is the other.
        stderr = io.StringIO()
        with session(step["session"]), contextlib.redirect_stderr(stderr):
            cli._registra_commento_proprio(1, step["comment"], {"progetto": step["project"], "errore": None})
        assert not stderr.getvalue(), f"the comment was not recorded: {stderr.getvalue()!r}"
        return None
    return step_command(cli, step, folder)


def record_family(rt, cli, steps, folder):
    """A family's sequence on a fresh folder and a fresh clock: for each step its outcome and every row after it."""
    if folder.exists():
        shutil.rmtree(folder)
    folder.mkdir(parents=True)
    os.environ["REGIA_RUNTIME"] = str(folder)
    pinned = pinned_datetime()
    clock = Clock(pinned)
    rt.datetime = pinned
    rt._adesso = lambda: clock.wall
    rt._mono = lambda: clock.mono
    rt._CLOCK = CLOCK_TAG
    cli.datetime = clocked_datetime(pinned, clock)
    db = folder / rt.DB_NOME
    out = []
    for step in steps:
        record = dict(step)
        if step["op"] == "advance":
            clock.advance(step["seconds"])
        else:
            record["outcome"] = family_op(rt, cli, step, folder)
            record["rows"] = rows_of(db) if db.exists() else []
        out.append(record)
    return out


def check_public(text, folders, sessions):
    """The fixture is public (#692 A): no path, user, machine or session of this run may be in it."""
    needles = {str(Path.home()), socket.gethostname(), *(str(Path(folder).resolve()) for folder in folders)}
    needles |= {os.environ.get(name) for name in ("USERNAME", "USER", "COMPUTERNAME")}
    needles |= set(sessions)
    found = [needle for needle in needles if needle and (needle in text or json.dumps(needle)[1:-1] in text)]
    assert not found, f"the families' fixture would carry {len(found)} value(s) of this machine or session"
    assert not re.search(r"[A-Za-z]:(\\\\|/)", text), "the families' fixture would carry a path of this machine"


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
    # This process's sessions reach neither the steps nor the file: the steps name theirs.
    sessions = [value for value in (os.environ.pop(name, None) for name in SESSION_VARIABLES) if value]
    sys.dont_write_bytecode = True  # nothing written next to the prototype
    scratch = Path(args.scratch or Path(args.dossier) / "runtime" / "store-record")
    rt = load_runtime(args.dossier)
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    iso = {**header(args, rt), "unit": "microseconds since the Unix epoch, UTC", "vectors": record_iso(rt)}
    (out / "iso_vectors.json").write_text(json.dumps(iso, ensure_ascii=True, indent=1) + "\n", encoding="utf-8", newline="\n")
    rows = {**header(args, rt), **record_rows(rt, scratch)}
    (out / "golden_rows.json").write_text(json.dumps(rows, ensure_ascii=True, indent=1) + "\n", encoding="utf-8", newline="\n")
    # `regia.py` reads its tracker's configuration when loaded: the clone's, never the live one's.
    os.environ["REGIA_HOME"] = str(Path(args.dossier).resolve())
    cli = load_cli(args.dossier, rt)
    folders = {name: scratch.with_name(f"{scratch.name}-{name}") for name in ("lease", "cursor", "step")}
    families = {
        **header(args, rt), "clock": {"start": START, "monotonic_start": MONO_START, "tag": CLOCK_TAG},
        "step_ttl_s": cli.PASSO_TTL_S,
        "families": {"lease": record_family(rt, cli, LEASE_STEPS, folders["lease"]),
                     "cursor": record_family(rt, cli, CURSOR_STEPS, folders["cursor"]),
                     "step": record_family(rt, cli, STEP_STEPS, folders["step"])},
    }
    text = json.dumps(families, ensure_ascii=True, indent=1) + "\n"
    check_public(text, [args.dossier, scratch, *folders.values()], sessions)
    (out / FAMILIES).write_text(text, encoding="utf-8", newline="\n")
    for folder in [scratch, *folders.values()]:
        shutil.rmtree(folder, ignore_errors=True)
    recorded = sum(len(steps) for steps in families["families"].values())
    print(f"{len(iso['vectors'])} vectors, {len(rows['steps'])} steps, {recorded} steps of the families, "
          f"Python {sys.version.split()[0]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
