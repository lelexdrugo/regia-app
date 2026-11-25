"""The prototype at a fixed instant, for `record` and for the local test that runs the binary on the prototype. Loads
`regia.py` from its path, as the prototype loads its own modules, fixes its clock and runs its `main`:

- `datetime.datetime` becomes a subclass whose `now`, `utcnow` and `fromtimestamp` answer the instant and whose local
  time is UTC, so no offset depends on the machine. Every module the prototype loads from here on (`regia.py`,
  `runtime.py`, `usage.py`, `controlli.py`, `dichiarazioni.py`) imports that class;
- `_oggi`, the one point `regia.py` reads today's date from, answers the instant's date;
- the monotonic clock of `runtime.py` stands still, so a lock or a lease written now expires exactly `ttl` seconds
  after the instant, and a remaining time reads the same on every run;
- the commits the prototype makes carry the instant as their dates;
- `time.strftime` with no time, which dates the scaffold of a new project (`{{DATA}}`), formats the instant, in UTC.

Nothing in the prototype changes and nothing is written next to it.

    python fixed_now.py <path of tools/regia.py> <instant, ISO 8601 with its offset> <argument>...
"""
import datetime as _datetime
import importlib.util
import os
import sys
import time as _time
import types

sys.dont_write_bytecode = True  # no __pycache__ in the prototype's tools/

script, instant, *arguments = sys.argv[1:]
_UTC = _datetime.timezone.utc
_REAL = _datetime.datetime
# Any value: only differences between two readings count, and every reading is this one.
_MONOTONIC = 1000.0


class _Fixed(_REAL):
    """A datetime whose clock reads the instant, and whose local time is UTC."""

    @classmethod
    def now(cls, tz=None):
        return INSTANT.astimezone(tz) if tz is not None else INSTANT.replace(tzinfo=None)

    @classmethod
    def utcnow(cls):
        return INSTANT.replace(tzinfo=None)

    @classmethod
    def fromtimestamp(cls, timestamp, tz=None):
        moment = super().fromtimestamp(timestamp, tz if tz is not None else _UTC)
        return moment if tz is not None else moment.replace(tzinfo=None)

    def astimezone(self, tz=None):
        aware = self if self.tzinfo is not None else self.replace(tzinfo=_UTC)
        return _REAL.astimezone(aware, tz if tz is not None else _UTC)

    def timestamp(self):
        return _REAL.timestamp(self if self.tzinfo is not None else self.replace(tzinfo=_UTC))


INSTANT = _Fixed.fromisoformat(instant.replace("Z", "+00:00"))
if INSTANT.tzinfo is None:
    sys.exit(f"the instant {instant} has no offset")
_datetime.datetime = _Fixed

spec = importlib.util.spec_from_file_location("prototype", script)
if spec is None or spec.loader is None:
    sys.exit(f"cannot load {script}")
prototype = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prototype)
setattr(prototype, "_oggi", lambda: INSTANT.date())


def _strftime(form, moment=None):
    """`time.strftime` at the instant, in UTC, when it is given no time; as it is otherwise."""
    return _time.strftime(form, INSTANT.astimezone(_UTC).timetuple() if moment is None else moment)


# The prototype's own `time`, with `strftime` at the instant: `sleep` and the rest stay the real ones.
_TIME = {name: getattr(_time, name) for name in dir(_time) if not name.startswith("__")}
_TIME["strftime"] = _strftime
setattr(prototype, "time", types.SimpleNamespace(**_TIME))

# `_runtime()` loads `runtime.py` afresh at every call: each copy gets the clock that stands still.
_load_runtime = prototype._runtime


def _runtime():
    module = _load_runtime()
    module._mono = lambda: _MONOTONIC
    return module


setattr(prototype, "_runtime", _runtime)
# `git commit` reads its dates from these, and `_git` passes the environment on.
os.environ["GIT_AUTHOR_DATE"] = os.environ["GIT_COMMITTER_DATE"] = f"@{int(INSTANT.timestamp())} +0000"
sys.argv = [script, *arguments]
prototype.main()
