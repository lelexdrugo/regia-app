"""Dev only: the prototype's answers on the moviola's fixtures, the Python side of `tests/moviola.rs`. It loads
`tools/moviola/` from the copy of the prototype in REGIA_HOME and changes nothing there or in the fixtures.

    python moviola_oracle.py <fixtures/moviola>

It reads `settings.json` of the fixtures and prints one JSON document: the portfolio (`serve._portafoglio`), each
project's view (`serve._vista`, the sidecars included), the web URL of each remote (`fonti.normalizza_remote`), and
for each range the window `esporta._intervallo` reads and the milestones of `alpha` inside it. Paths on this machine
are written relative to the fixtures' folder, as `<root>/...` with `/`, so a recording holds on every machine.
"""
import json
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(os.environ["REGIA_HOME"]) / "tools"))

from moviola import dati, esporta, fonti, serve, vista  # noqa: E402
from moviola.util import parse_ts  # noqa: E402

FMT = "%Y-%m-%dT%H:%M:%SZ"


def relative(value, root):
    """Every string under the fixtures' folder, written relative to it."""
    if isinstance(value, dict):
        return {key: relative(item, root) for key, item in value.items()}
    if isinstance(value, list):
        return [relative(item, root) for item in value]
    if isinstance(value, str) and value.startswith(root):
        return "<root>" + value[len(root):].replace("\\", "/")
    return value


def window(start, end, folder):
    try:
        begin, finish = esporta._intervallo(start, end)
    except esporta.ErroreEsporta:
        return {"error": True}
    kept = [m["id"] for m in dati.milestones(folder)
            if (begin is None or parse_ts(m["ts"]) >= begin) and (finish is None or parse_ts(m["ts"]) < finish)]
    return {"from": begin.strftime(FMT) if begin else None, "to": finish.strftime(FMT) if finish else None, "kept": kept}


def main():
    root = Path(sys.argv[1]).resolve()
    settings = json.loads((root / "settings.json").read_text(encoding="utf-8"))
    folders = {p["id"]: str(root / p["folder"]) for p in settings["projects"]}
    # As `moviola serve` reads the registry: a title only where it is not empty.
    titles = {p["id"]: p["title"] for p in settings["projects"] if p["title"]}
    views = {}
    for project, folder in folders.items():
        link = settings["links"][project]
        config = {key: link[key] for key in ("owner", "repo", "project_url") if link[key] is not None}
        views[project] = serve._vista(folder, project, vista.Rimandi(folder, config=config, remote=link["remote"] or False))
    out = {
        "portfolio": serve._portafoglio(folders, titles),
        "views": views,
        "remotes": [[url, fonti.normalizza_remote(url)] for url in settings["remotes"]],
        "ranges": [[start, end, window(start, end, folders["alpha"])] for start, end in settings["ranges"]],
    }
    sys.stdout.reconfigure(encoding="utf-8")
    print(json.dumps(relative(out, str(root)), ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
