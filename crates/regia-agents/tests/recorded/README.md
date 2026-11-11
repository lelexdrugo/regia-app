# Recorded answers of the usage endpoints

Synthetic: written by hand in the shape the prototype's probes read (`tools/usage.py` of the dossier,
`_claude_finestra` and `_finestre_codex`), with the fields the real answers carry besides. No real account,
token or plan is in them. The tests put the credentials next to them at run time, with canary tokens, and
never reach the real endpoints.

- `claude-usage.json`: `api/oauth/usage`, the five-hour and seven-day windows at 14 % and 58 %, read at
  2026-11-11T02:00:00Z.
- `codex-usage.json`: `backend-api/wham/usage`, plan `plus`, the five-hour window at 0 % and the seven-day
  one at 20 %.
