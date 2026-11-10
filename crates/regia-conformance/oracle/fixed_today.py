"""The prototype on a fixed day, for `record`. Loads `regia.py` from its path, as the prototype loads its own
modules, replaces `_oggi`, the one point the prototype reads today's date from, and runs its `main`. Nothing in the
prototype changes and nothing is written next to it.

    python fixed_today.py <path of tools/regia.py> <YYYY-MM-DD> <argument>...
"""
import importlib.util
import sys
from datetime import date

sys.dont_write_bytecode = True  # no __pycache__ in the prototype's tools/

script, day, *arguments = sys.argv[1:]
fixed = date.fromisoformat(day)
spec = importlib.util.spec_from_file_location("prototype", script)
if spec is None or spec.loader is None:
    sys.exit(f"cannot load {script}")
prototype = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prototype)
setattr(prototype, "_oggi", lambda: fixed)
sys.argv = [script, *arguments]
prototype.main()
