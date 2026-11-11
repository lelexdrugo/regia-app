"""Dev only: the prototype's usage probes at a fixed instant, on the recorded answers, for the golden of
`budget` (Acceptance 1 of #463). It loads `tools/usage.py` from the dossier and changes nothing there:

- `datetime` in that module becomes a subclass whose `now` answers the instant and whose local time is UTC,
  as `regia-conformance/oracle/fixed_now.py` does, and `time.time` answers the instant;
- `_get_json` answers each endpoint from `crates/regia-agents/tests/recorded/`, so no endpoint is asked;
- the credentials are written here, under `--dir`, with tokens that are not tokens.

    python budget_oracle.py --dossier <dossier> --dir <folder under target/> > tests/golden/budget-python.json

It prints `usage.sonda()` as the prototype's `regia budget --json` prints it.
"""
import argparse
import base64
import datetime as _datetime
import importlib.util
import json
import os
import sys
from pathlib import Path

sys.dont_write_bytecode = True

NOW = 1_794_362_400  # 2026-11-11T02:00:00Z, the instant of the native test
RECORDED = Path(__file__).resolve().parents[3] / "regia-agents" / "tests" / "recorded"
_UTC = _datetime.timezone.utc
_REAL = _datetime.datetime


class _Fixed(_REAL):
    @classmethod
    def now(cls, tz=None):
        moment = _REAL.fromtimestamp(NOW, _UTC)
        return cls.fromtimestamp(NOW, tz) if tz is not None else cls(*moment.timetuple()[:6], tzinfo=_UTC)

    def astimezone(self, tz=None):
        aware = self if self.tzinfo is not None else self.replace(tzinfo=_UTC)
        return _REAL.astimezone(aware, tz if tz is not None else _UTC)


def jwt(exp):
    def part(doc):
        return base64.urlsafe_b64encode(json.dumps(doc, separators=(",", ":")).encode()).decode().rstrip("=")
    return f"{part({'alg': 'none'})}.{part({'exp': exp, 'canary': 'not-a-token'})}.c2lnbmF0dXJl"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--dossier", required=True)
    parser.add_argument("--dir", required=True)
    args = parser.parse_args()
    folder = Path(args.dir)
    folder.mkdir(parents=True, exist_ok=True)
    claude, codex = folder / ".credentials.json", folder / "auth.json"
    claude.write_text(json.dumps({"claudeAiOauth": {"accessToken": "not-a-token", "expiresAt": (NOW + 8 * 3600) * 1000,
                                                    "subscriptionType": "max"}}), encoding="utf-8")
    token = jwt(NOW + 3 * 24 * 3600)
    codex.write_text(json.dumps({"tokens": {"access_token": token, "account_id": "account-0001"}}), encoding="utf-8")
    os.environ["REGIA_CLAUDE_CREDENTIALS"], os.environ["REGIA_CODEX_AUTH"] = str(claude), str(codex)

    spec = importlib.util.spec_from_file_location("prototype_usage", Path(args.dossier) / "tools" / "usage.py")
    assert spec is not None and spec.loader is not None, "cannot load tools/usage.py"
    usage = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(usage)
    usage.datetime = _Fixed
    usage.time.time = lambda: float(NOW)
    answers = {usage.CLAUDE_URL: "claude-usage.json", usage.CODEX_URL: "codex-usage.json"}
    usage._get_json = lambda url, headers: json.loads((RECORDED / answers[url]).read_text(encoding="utf-8"))
    print(json.dumps(usage.sonda(), ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
