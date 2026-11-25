"""Dev only: the prototype's brief on a copy of the state, the Python side of the X of #468
(`tests/controller_cross.rs`): the state file the Rust resident wrote, read by the Python brief. It loads the
dossier's `tools/regia.py` and changes nothing there. The tracker is not read: `filtra` answers nothing, as in the
prototype's `tools/tests/test_brief_controller.py`, and Codex's token is a file that does not exist (#569). It
refuses the live state folder.

    python -X utf8 -I -B brief_oracle.py --dossier <dossier> --dir <folder>

One JSON line on stdout: the brief's warnings of the controller, its text and its JSON.
"""
import argparse
import contextlib
import io
import json
import os
import sys
from pathlib import Path
from unittest import mock


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
    os.environ["REGIA_CODEX_AUTH"] = str(folder / "no-codex-auth.json")
    sys.path.insert(0, str(Path(a.dossier) / "tools"))
    import regia  # noqa: E402

    def brief(as_json):
        out = io.StringIO()
        with mock.patch.object(regia, "filtra", return_value=[]), contextlib.redirect_stdout(out):
            regia.cmd_brief(argparse.Namespace(json=as_json))
        return out.getvalue()

    result = {"warnings": regia.avvisi_controller(), "text": brief(False), "json": json.loads(brief(True))}
    print(json.dumps(result), flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
