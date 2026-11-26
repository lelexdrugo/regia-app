"""Dev only: the prototype's `consolida`, `store` and `report` on synthetic cases, the Python side of
`tests/moviola_consolidate.rs`. It loads `tools/moviola/` from the copy of the prototype in REGIA_HOME and changes
nothing there.

    python consolidate_oracle.py <work folder>

Each case is a list of steps, run in a folder of its own under the work folder: a dossier (`project/`) and stores
named by the step. The cases are the prototype's tests of `test_moviola.py` (`TestReport`, `TestStore`,
`TestConsolida`) and `test_moviola_viewer.py` (`TestSessioneCitata`), with invented projects, sessions and
folders, then cases of this port's own: the edges of the reader of reports, of the store's files and of the rows
already written. After each step the case records what the step gave (or the name of the exception it raised, and
then it stops) and every file of its folder, as text. The instants the store writes from its clock (`ts` in
`consolidati.json`) are written `<ts>`; paths are relative to the case's folder, with `/`. It prints one JSON document.
"""
import json
import os
import re
import shutil
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(os.environ["REGIA_HOME"]) / "tools"))

from moviola import consolida, report, store  # noqa: E402

SCHEMA = "moviola.attivita/v0"
DAY = "2026-10-19T00:00:00Z"
LATER = "2026-10-22T00:00:00Z"
CHAT = "6c1f0e2a-3b4d-4e5f-8a9b-0c1d2e3f4a5b"
STAMP = re.compile(r'"ts": "\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z"')


def rec_store(ctx="ctx_1", t="2026-10-14T10:00:00Z", **kw):
    """`TestStore.rec`."""
    r = {"schema": SCHEMA, "ts_inizio": t, "ts_fine": None, "progetto": "alpha", "task": 1, "ruolo": "worker",
         "agente": "claude", "modello": "sonnet", "effort": "high", "sessione": "s", "lanciata_da": None,
         "fonte": "orca-dispatch", "dispatch": ctx, "transcript": None, "token": None, "esito": "done"}
    r.update(kw)
    return r


def rec(d, task, t, **kw):
    """`TestConsolida.rec`."""
    r = {"schema": SCHEMA, "ts_inizio": t, "ts_fine": kw.pop("ts_fine", None), "progetto": "alpha", "task": task,
         "ruolo": "worker", "agente": "claude", "modello": "sonnet", "effort": "high", "sessione": "s" + str(d),
         "lanciata_da": None, "fonte": "orca-dispatch", "dispatch": d, "transcript": None, "token": None,
         "esito": "done"}
    r.update(kw)
    return r


def rec_chat(**kw):
    """`TestSessioneCitata.rec`."""
    r = {"schema": SCHEMA, "ts_inizio": "2026-10-19T15:00:45Z", "ts_fine": "2026-10-21T10:39:01Z",
         "progetto": "alpha", "task": None, "ruolo": "interattiva", "agente": "claude", "modello": "claude-opus-5-5",
         "effort": None, "sessione": CHAT, "lanciata_da": None, "fonte": "orca-usage-claude", "dispatch": None,
         "transcript": None, "token": {"in": 584, "out": 197651}, "esito": None}
    r.update(kw)
    return r


def front(name, text):
    """A report as the tests write it: the front matter only."""
    return {"op": "report", "name": name, "text": "---\n" + text + "\n---\n"}


def upsert(records, now=DAY, store_name="store", remove=None):
    step = {"op": "upsert", "store": store_name, "records": records, "now": now}
    if remove is not None:
        step["remove"] = remove
    return step


def consolidate(now=DAY, store_name="store", project="alpha"):
    return {"op": "consolidate", "store": store_name, "project": project, "now": now}


def read(store_name="store"):
    return {"op": "read", "store": store_name}


def consolidated(store_name="store"):
    return {"op": "consolidated", "store": store_name}


def prune(now=DAY, store_name="store"):
    return {"op": "prune", "store": store_name, "now": now}


def mark(task, records, store_name="store", project="alpha"):
    return {"op": "mark", "store": store_name, "project": project, "task": task, "records": records}


REPORTS = {"op": "mkdir", "path": "project/.regia/report"}

# What the prototype's Orca adapter gives on the synthetic Orca files of two cases of `TestStore` and
# `TestConsolida`, with these invented folders: the adapter is the collection's, which this port leaves out.
SYNC = [
    {"schema": SCHEMA, "ts_inizio": "2026-10-14T17:43:17Z", "ts_fine": "2026-10-14T17:46:53Z", "progetto": "alpha",
     "attribuzione": "dispatch", "task": 2, "ruolo": "worker", "agente": "claude", "modello": "sonnet", "effort": "high",
     "sessione": "orca-ctx_1", "lanciata_da": "orch-1", "fonte": "orca-dispatch", "dispatch": "ctx_1",
     "transcript": "C:\\u\\.claude\\projects\\x\\a1.jsonl", "token": {"in": 10, "out": 100}, "esito": "done",
     "retry_of": None},
    {"schema": SCHEMA, "ts_inizio": "2026-10-13T10:00:00Z", "ts_fine": "2026-10-13T11:00:00Z", "progetto": "alpha",
     "attribuzione": "dedotta", "task": None, "ruolo": "interattiva", "agente": "claude", "modello": "claude-opus-5-5",
     "effort": None, "sessione": "chat", "lanciata_da": None, "fonte": "orca-usage-claude", "dispatch": None,
     "transcript": "C:\\u\\.claude\\projects\\x\\chat.jsonl", "token": {"in": 10, "out": 100}, "esito": None},
]
RESERVED = [
    {"schema": SCHEMA, "ts_inizio": "2026-10-14T17:43:17Z", "ts_fine": "2026-10-14T17:46:53Z", "progetto": "alpha",
     "attribuzione": "dispatch", "task": 2, "ruolo": "worker", "agente": "claude", "modello": "sonnet", "effort": "high",
     "sessione": "orca-ctx_1", "lanciata_da": "orch-1", "fonte": "orca-dispatch", "dispatch": "ctx_1",
     "transcript": "orca-archivio:ctx_1", "token": None, "esito": "done", "retry_of": None},
]

TESTO = '''---
tipo: done
task: 12
riassunto: "Frase: con due punti, e # cancelletto"
prove:
  - "tools/x.py"
round: 2                       # commento
sessione: "ctx_abc"
sottoagenti: 3
controlli:
  - tipo: revisione-avversaria
    modello: "codex/gpt"
    esito: difetti
    difetti: [D1, D2, "D3"]
  - tipo: test
    esito: ok
difetti_chiusi: ["#12/D1", '#12/D2']
---
Corpo libero con `round: 99` che non conta.
'''

# One report for each model of `contratti.md` («Report del worker»), as a worker fills it.
DONE = '''---
tipo: done
task: 41
riassunto: "Ho portato il lettore dei report; ho trovato due casi limite; resta il comando."
prove: ["crates/x/src/lettore.rs", "cargo test -p x → verde"]
modello: "claude opus 5.5, effort massimo: il Mandato lo chiede"
resta:
  - "V1: il comando della riga di comando → #42"
  - "V2: la riga dell'inventario → ?"
stop_reason: objective_met
round: 2
sessione: "ctx_0123456789ab"
sottoagenti: 1
branch: "worker/41-lettore@0123abc"
worktree: "<worktree>"
controlli:
  - tipo: revisione-avversaria
    modello: "codex/gpt"
    esito: difetti
    difetti: [D1, D2]
  - tipo: test
    modello: "claude/opus"
    esito: ok
    difetti: []
difetti_chiusi: ["#40/D1"]
---
Il corpo è libero.
'''
GATE = '''---
tipo: gate
task: 42
riassunto: "Il comando è pronto fino al nome; il nome manca dalla tabella; resta la scelta."
prove: ["crates/x/src/comando.rs"]
modello: "claude opus 5.5"
stop_reason: decision_needed
domanda: "Quale nome per il comando?"
opzioni: ["A: uno", "B: due"]
consiglio: "B, perché è più corto"
se_silenzio: fermo
round: 1
sessione: "ctx_fedcba987654"
sottoagenti: 0
branch: "worker/42-comando@4567def"
worktree: "<worktree>"
---
'''
PROPOSAL = '''---
tipo: proposal
task: 43
riassunto: "Serve un task nuovo per la cache; non l'ho eseguito; resta la decisione."
prove: ["docs/nota.md"]
modello: "claude sonnet 5.5"
relazione: consecutiva
bozza_spec: "Portare la cache dei report, con i suoi test."
round: 1
sessione: "ctx_00aa11bb22cc"
sottoagenti: 0
---
'''
FAILED = '''---
tipo: failed
task: 44
riassunto: "Il push non è riuscito; il lavoro è nel commit locale; resta il push."
prove: ["git push → rifiutato"]
modello: "claude opus 5.5"
stop_reason: runtime_error
causa: "il remoto ha rifiutato il push"
ripresa: "rilanciare il push dallo stesso worktree"
round: 3
sessione: "ctx_33dd44ee55ff"
sottoagenti: 2
branch: "worker/44-push@89ab012"
worktree: "<worktree>"
controlli:
  - tipo: rilettura
    esito: non-concluso
---
'''

CASES = [
    # ---------- TestReport ----------
    {"name": "report-front-matter-complete", "from": "TestReport.test_front_matter_completo",
     "steps": [{"op": "front_matter", "text": TESTO}]},
    {"name": "report-closed-defects-in-a-block-and-no-fields", "from": "TestReport.test_difetti_chiusi_a_blocchi_e_report_senza_campi",
     "steps": [{"op": "front_matter", "text": "---\ntipo: done\ntask: 2\ndifetti_chiusi:\n  - \"#12/D1\"\n  - \"#12/D2\"\n---\n"},
               {"op": "front_matter", "text": "niente front matter"}]},
    {"name": "report-absent-is-not-zero", "from": "TestReport.test_leggi_report_distingue_assente_da_zero",
     "steps": [REPORTS,
               {"op": "report", "name": "2026-10-14-2-vecchio.md", "text": "---\ntipo: done\ntask: 2\n---\n"},
               {"op": "report", "name": "2026-10-19-37-nuovo.md",
                "text": "---\ntipo: done\ntask: 37\nround: 1\nsessione: \"s\"\nsottoagenti: 0\n---\n"},
               {"op": "reports"}]},
    {"name": "report-stop-reason", "from": "TestReport.test_leggi_report_espone_stop_reason_e_senza_il_campo_resta_none",
     "steps": [REPORTS,
               {"op": "report", "name": "2026-11-03-251-con.md",
                "text": "---\ntipo: gate\ntask: 251\nstop_reason: decision_needed   # una scelta del titolare\n---\n"},
               {"op": "report", "name": "2026-10-19-37-senza.md", "text": "---\ntipo: done\ntask: 37\nround: 1\n---\n"},
               {"op": "reports"}]},
    {"name": "report-the-four-models-of-the-contract", "from": "contratti.md, the models of the worker's report",
     "steps": [REPORTS,
               {"op": "report", "name": "2026-11-20-41-lettore.md", "text": DONE},
               {"op": "report", "name": "2026-11-21-42-comando.md", "text": GATE},
               {"op": "report", "name": "2026-11-22-43-cache.md", "text": PROPOSAL},
               {"op": "report", "name": "2026-11-23-44-push.md", "text": FAILED},
               {"op": "front_matter", "text": DONE}, {"op": "front_matter", "text": GATE},
               {"op": "front_matter", "text": PROPOSAL}, {"op": "front_matter", "text": FAILED},
               {"op": "reports"},
               consolidate(now="2026-11-26T00:00:00Z", store_name="none")]},

    # ---------- TestStore ----------
    {"name": "store-upsert-is-idempotent", "from": "TestStore.test_upsert_idempotente",
     "steps": [upsert([rec_store("ctx_1"), rec_store("ctx_2", t="2026-09-30T10:00:00Z")]),
               upsert([rec_store("ctx_1"), rec_store("ctx_2", t="2026-09-30T10:00:00Z")]),
               read()]},
    {"name": "store-a-running-dispatch-is-replaced", "from": "TestStore.test_un_dispatch_in_corso_si_sostituisce",
     "steps": [upsert([rec_store("ctx_1", esito="in-corso")]),
               upsert([rec_store("ctx_1", esito="done", ts_fine="2026-10-14T11:00:00Z")]),
               read()]},
    {"name": "store-an-absorbed-session-is-removed", "from": "TestStore.test_rimuovi_sessione_assorbita",
     "steps": [upsert([rec_store(None, dispatch=None, fonte="orca-usage-claude", sessione="abc", ruolo="interattiva", task=None)]),
               upsert([], remove=["orca-usage-claude:abc"]),
               read()]},
    {"name": "store-the-same-records-twice", "from": "TestStore.test_sync_due_volte_stesso_numero_di_record (the adapter's records)",
     "steps": [upsert(SYNC, remove=["orca-usage-claude:a1"]),
               upsert(SYNC, remove=["orca-usage-claude:a1"]),
               read()]},
    {"name": "store-retention-and-consolidation", "from": "TestStore.test_ritenzione_non_riammette_ne_elimina_senza_consolidamento",
     "steps": [upsert([rec_store("ctx_old", t="2026-06-01T10:00:00Z", task=7),
                       rec_store(None, t="2026-06-01T10:00:00Z", dispatch=None, fonte="orca-usage-claude", sessione="old",
                                 task=None, progetto=None, ruolo="interattiva"),
                       rec_store("ctx_new", t="2026-10-10T10:00:00Z", task=8)]),
               prune(),
               mark(7, [rec_store("ctx_old", t="2026-06-01T10:00:00Z", task=7)]),
               prune(),
               upsert([rec_store("ctx_old", t="2026-06-01T10:00:00Z", task=7)]),
               read()]},
    {"name": "store-a-late-record-holds-the-pruning", "from": "TestStore.test_un_record_nuovo_dopo_il_consolidamento_blocca_la_potatura",
     "steps": [upsert([rec_store("ctx_a", t="2026-06-01T10:00:00Z", task=7)]),
               mark(7, [rec_store("ctx_a", t="2026-06-01T10:00:00Z", task=7)]),
               upsert([rec_store("ctx_b", t="2026-06-02T10:00:00Z", task=7)]),
               upsert([rec_store("ctx_c", t="2026-10-18T10:00:00Z", task=7)]),
               prune(),
               read()]},

    # ---------- TestConsolida ----------
    {"name": "consolidate-with-and-without-process-fields", "from": "TestConsolida.test_aggregato_con_e_senza_campi_di_processo",
     "steps": [REPORTS,
               upsert([rec("ctx_2", 2, "2026-10-14T17:43:17Z", ts_fine="2026-10-14T17:46:53Z", token={"in": 38, "out": 18404}),
                       rec("ctx_12", 12, "2026-10-19T13:29:06Z", ts_fine="2026-10-19T13:38:45Z", agente="codex", modello=None, effort=None),
                       rec("ctx_27", 27, "2026-10-19T15:04:58Z", ts_fine="2026-10-19T15:29:27Z", token={"in": 5, "out": 7}),
                       rec("ctx_27b", 27, "2026-10-19T16:00:00Z", ts_fine="2026-10-19T16:10:00Z", token={"in": 1, "out": 1}, retry_of="ctx_27"),
                       rec("ctx_x", 99, "2026-10-19T09:00:00Z", progetto="beta"),
                       rec(None, None, "2026-10-19T09:00:00Z", dispatch=None, fonte="orca-usage-claude", sessione="chat", ruolo="interattiva")]),
               {"op": "report", "name": "2026-10-14-2-a.md", "text": "---\ntipo: done\ntask: 2\n---\n"},
               {"op": "report", "name": "2026-10-19-12-b.md",
                "text": "---\ntipo: done\ntask: 12\nround: 1\nsottoagenti: 0\ncontrolli:\n  - tipo: revisione-avversaria\n"
                        "    modello: codex\n    esito: difetti\n    difetti: [D1, D2, D3]\n---\n"},
               {"op": "report", "name": "2026-10-19-27-c.md",
                "text": "---\ntipo: done\ntask: 27\nround: 2\nsottoagenti: 1\ndifetti_chiusi: [\"#12/D1\", \"#12/D2\"]\n---\n"},
               consolidate(), consolidated()]},
    {"name": "consolidate-a-task-with-reports-only", "from": "TestConsolida.test_task_con_solo_il_report_ha_la_sua_riga",
     "steps": [REPORTS,
               front("2026-10-19-37-a.md", "tipo: done\ntask: 37\nround: 1\nsessione: s\nsottoagenti: 0"),
               front("2026-10-19-38-b.md", "tipo: done\ntask: 38"),
               front("2026-10-19-39-c.md", "tipo: done\ntask: 39\ncontrolli:\n  - tipo: rilettura\n    esito: ok\n    difetti: []"),
               consolidate(), consolidated(), consolidate()]},
    {"name": "consolidate-a-review-in-session-counts-what-another-task-closed",
     "from": "TestConsolida.test_revisione_fatta_in_sessione_conta_i_difetti_chiusi_da_un_altro_task",
     "steps": [REPORTS,
               front("2026-10-19-40-a.md", "tipo: done\ntask: 40\nround: 1\ncontrolli:\n  - tipo: revisione-avversaria\n    esito: difetti\n    difetti: [D1, D2]"),
               front("2026-10-20-41-b.md", 'tipo: done\ntask: 41\nround: 1\ndifetti_chiusi: ["#40/D1"]'),
               consolidate()]},
    {"name": "consolidate-reports-only-does-not-bypass-a-running-task",
     "from": "TestConsolida.test_solo_report_non_aggira_il_rinvio_di_un_task_in_corso",
     "steps": [REPORTS, upsert([rec("a", 6, "2026-10-11T10:00:00Z", esito="in-corso")]),
               front("2026-10-19-6-a.md", "tipo: done\ntask: 6\nround: 1"), consolidate()]},
    {"name": "consolidate-a-row-of-expired-records-stays", "from": "TestConsolida.test_riga_nata_dal_grezzo_scaduto_non_diventa_solo_report",
     "steps": [REPORTS, upsert([rec("old", 7, "2026-06-01T10:00:00Z", ts_fine="2026-06-01T11:00:00Z")]), consolidate(),
               front("2026-06-01-7-a.md", "tipo: done\ntask: 7\nround: 3"), consolidate()]},
    {"name": "consolidate-closes-on-the-fixing-task", "from": "TestConsolida.test_difetti_chiude_nel_task_di_correzione",
     "steps": [REPORTS,
               upsert([rec("c", 27, "2026-10-19T15:04:58Z", ts_fine="2026-10-19T15:29:27Z"),
                       rec("r", 12, "2026-10-19T13:29:06Z", ts_fine="2026-10-19T13:38:45Z")]),
               front("2026-10-19-12-b.md", "tipo: done\ntask: 12\nround: 1\ncontrolli:\n  - tipo: revisione-avversaria\n    esito: difetti\n    difetti: [D1, D2]"),
               front("2026-10-19-27-c.md", 'tipo: done\ntask: 27\nround: 1\ndifetti_chiusi: ["#12/D2", "#12/D1", "#12/D1", "D3"]'),
               consolidate()]},
    {"name": "consolidate-cache-tokens-only-where-they-are", "from": "TestConsolida.test_token_di_cache_si_sommano_solo_dove_ci_sono",
     "steps": [REPORTS,
               upsert([rec("a", 5, "2026-10-10T10:00:00Z", token={"in": 1, "out": 2, "cache_lettura": 10}),
                       rec("b", 5, "2026-10-11T10:00:00Z", token={"in": 1, "out": 2, "cache_lettura": 5, "cache_scrittura": 3}),
                       rec("c", 6, "2026-10-11T10:00:00Z", token={"in": 4, "out": 4})]),
               consolidate()]},
    {"name": "consolidate-round-counts-the-dispatches", "from": "TestConsolida.test_round_senza_report_conta_i_dispatch",
     "steps": [REPORTS, upsert([rec("a", 5, "2026-10-10T10:00:00Z"), rec("b", 5, "2026-10-11T10:00:00Z")]), consolidate()]},
    {"name": "consolidate-a-running-task-is-deferred", "from": "TestConsolida.test_task_con_dispatch_in_corso_si_rinvia",
     "steps": [REPORTS, upsert([rec("a", 5, "2026-10-10T10:00:00Z"), rec("b", 6, "2026-10-11T10:00:00Z", esito="in-corso")]),
               consolidate(), consolidated()]},
    {"name": "consolidate-again-and-expired-rows-are-kept",
     "from": "TestConsolida.test_riesecuzione_stesso_file_e_righe_di_task_scaduti_si_conservano",
     "steps": [REPORTS,
               upsert([rec("old", 7, "2026-06-01T10:00:00Z", ts_fine="2026-06-01T11:00:00Z"), rec("n", 8, "2026-10-10T10:00:00Z")]),
               consolidate(), read(), consolidate()]},
    {"name": "consolidate-nothing-reserved", "from": "TestConsolida.test_aggregati_e_store_senza_contenuti_riservati (the adapter's records)",
     "steps": [REPORTS, upsert(RESERVED), consolidate()]},

    # ---------- TestSessioneCitata (test_moviola_viewer.py) ----------
    {"name": "cited-the-orchestrator-s-chat-goes-to-the-tasks-that-name-it",
     "from": "TestSessioneCitata.test_la_chat_dell_orchestratore_va_ai_task_che_la_citano",
     "steps": [REPORTS,
               upsert([rec_chat(), rec_chat(sessione=CHAT, task=99, ruolo="worker", dispatch="ctx_99", fonte="orca-dispatch",
                                            modello="sonnet", esito="done")], now=LATER, store_name="attivita"),
               front("2026-10-19-37-a.md", f'tipo: done\ntask: 37\nround: 1\nsessione: "{CHAT}"\nsottoagenti: 0'),
               front("2026-10-21-26-b.md", f'tipo: done\ntask: 26\nround: 2\nsessione: "{CHAT}"\ncontrolli:\n'
                     '  - tipo: test\n    esito: ok\n    difetti: []'),
               front("2026-10-19-22-c.md", "tipo: done\ntask: 22\nround: 1"),
               consolidate(now=LATER, store_name="attivita"), read("attivita")]},
    {"name": "cited-a-dispatch-of-the-task-and-a-named-session", "from": "TestSessioneCitata.test_dispatch_del_task_piu_sessione_citata",
     "steps": [REPORTS,
               upsert([rec_chat(), rec_chat(task=5, ruolo="worker", dispatch="ctx_5", sessione="w5", fonte="orca-dispatch",
                                            modello="claude-sonnet-5-5", effort="high", token={"in": 1, "out": 2}, esito="done")],
                      now=LATER, store_name="attivita"),
               front("2026-10-20-5-a.md", f'tipo: done\ntask: 5\nround: 1\nsessione: "{CHAT}"'),
               consolidate(now=LATER, store_name="attivita")]},
    {"name": "cited-a-dispatch-without-a-task-by-its-id", "from": "TestSessioneCitata.test_un_dispatch_senza_task_si_cita_con_il_suo_id",
     "steps": [REPORTS,
               upsert([rec_chat(ruolo="worker", dispatch="ctx_ab", sessione="w", fonte="orca-dispatch", modello="sonnet",
                                effort="high", esito="done")], now=LATER, store_name="attivita"),
               front("2026-10-20-8-a.md", "tipo: done\ntask: 8\nround: 1\nsessione: ctx_ab"),
               consolidate(now=LATER, store_name="attivita")]},
    {"name": "cited-the-link-outlives-the-store-and-the-row-stays-open",
     "from": "TestSessioneCitata.test_il_legame_sopravvive_alla_scadenza_e_la_riga_resta_aggiornabile",
     "steps": [REPORTS, upsert([rec_chat()], now=LATER, store_name="attivita"),
               front("2026-10-21-26-b.md", f'tipo: done\ntask: 26\nround: 1\nsessione: "{CHAT}"'),
               consolidate(now=LATER, store_name="attivita"), consolidate(now=LATER, store_name="attivita"),
               front("2026-10-22-26-c.md", "tipo: done\ntask: 26\nround: 2\ncontrolli:\n  - tipo: test\n    esito: ok"),
               consolidate(now=LATER, store_name="attivita-vuota")]},

    # ---------- this port's own cases ----------
    {"name": "own-no-store-reports-only", "from": "Acceptance 2 of #491: no raw store, no error, the report-only branch",
     "steps": [REPORTS,
               front("2026-11-02-51-a.md", "tipo: done\ntask: 51\nround: 1\nsessione: ctx_51\nsottoagenti: 2"),
               front("2026-11-03-52-b.md", "tipo: done\ntask: 52"),
               consolidate(now="2026-11-26T00:00:00Z", store_name="missing"), consolidated("missing"),
               consolidate(now="2026-11-26T00:00:00Z", store_name="missing")]},
    {"name": "own-no-dossier-folder", "from": "a project whose folder has nothing yet: the empty aggregates are written",
     "steps": [consolidate(now="2026-11-26T00:00:00Z", store_name="missing")]},
    {"name": "own-rows-as-written", "from": "_leggi_righe: the lines it keeps, by Python's int(), and those it drops",
     "steps": [{"op": "write", "path": "project/.regia/moviola/aggregati.jsonl", "text":
                '{"task": 3, "schema": "moviola.aggregato/v0", "progetto": "alpha", "nota": "è tenuta"}\n'
                "not json\n[1, 2]\n"
                '{"task": "5", "round": 1}\n{"task": 2.9, "round": 2.50}\n{"round": 9}\n'
                '{"task": 3, "round": 4, "modelli": [], "x": {"b": 1, "a": [1e3, -0.0, 1.5e-07]}}\n'
                '{"task": true, "x": "\\u00e8\\ud83d\\ude00\\u0007"}\n   \n{"task": null}\n{"task": "x"}\n{"task": " 6 "}\r\n'},
               consolidate(now="2026-11-26T00:00:00Z", store_name="missing")]},
    {"name": "own-a-row-from-reports-alone-keeps-its-cited-entries",
     "from": "_con_citate_precedenti on a row the store no longer feeds",
     "steps": [{"op": "write", "path": "project/.regia/moviola/aggregati.jsonl", "text":
                '{"schema": "moviola.aggregato/v0", "progetto": "alpha", "task": 26, "finestra": null, "round": 1, '
                '"sottoagenti_dichiarati": null, "modelli": [{"agente": "claude", "modello": "m1", "effort": null, '
                '"sessioni": 1, "via": "report"}, {"agente": "codex", "modello": "m2", "effort": "high", "sessioni": 2, '
                '"via": "report", "extra": [1]}], "controlli": [], "difetti": {"trovati": null, "chiusi": null}, "token": null}\n'},
               REPORTS,
               upsert([rec_chat(sessione="c1", modello="m1")], now=LATER),
               front("2026-10-21-26-b.md", 'tipo: done\ntask: 26\nround: 3\nsessione: "c1"'),
               consolidate(now=LATER)]},
    {"name": "own-reports-the-edges-of-the-subset", "from": "front_matter and leggi_report on the edges of the YAML subset",
     "steps": [REPORTS,
               {"op": "report", "name": "2026-10-01-60-bom-crlf.md",
                "text": "\ufeff---\r\ntipo: done\r\ntask: 60\r\nround: 2 # nota\r\nsottoagenti: \"3\"\r\n---\r\ncorpo\r\n"},
               {"op": "report", "name": "2026-10-02-61-quotes.md",
                "text": "---\ntipo: 'it''s done'\ntask: 61\nsessione: \"a # b\" # c\nround: \u0663\ncontrolli:\n"
                        "  # a comment\n  - tipo: test\n    esito: ok\n    difetti: D12\n    modello: ~\n"
                        "  - just a scalar\n  - tipo: rilettura\n    difetti: [\"D1, D2\", 'D3', , 4]\n"
                        "    tipo: rilettura-due\n  -\nround: 5\ndifetti_chiusi:\n  - [a, b]\n  - \"#9/D2\"\n  - 7\n---\n"},
               {"op": "report", "name": "2026-10-03-62-no-task-field.md", "text": "---\nround: 1\ncontrolli: x\n---\n"},
               {"op": "report", "name": "2026-10-04-x-no-number.md", "text": "---\ntipo: done\nround: 1\n---\n"},
               {"op": "report", "name": "2026-10-05-63-never-closed.md", "text": "---\ntipo: done\ntask: 63\nround: 1\n"},
               {"op": "report", "name": "2026-10-06-64-no-front.md", "text": "tipo: done\ntask: 64\n"},
               {"op": "report", "name": "2026-10-07-65-task-text.md",
                "text": "---\ntask: \" 66 \"\nround: true\nsottoagenti: [1]\nstop_reason: [a]\ncontrolli: 0\nkey:value\n---\n"},
               {"op": "report", "name": "notes.txt", "text": "---\ntask: 70\nround: 1\n---\n"},
               {"op": "mkdir", "path": "project/.regia/report/2026-10-08-71-a-folder.md"},
               {"op": "reports"},
               consolidate(now="2026-11-26T00:00:00Z", store_name="missing")]},
    {"name": "own-a-check-that-is-a-number", "from": "leggi_report raises on `controlli: 3`, which Python cannot walk",
     "steps": [REPORTS, front("2026-10-01-80-a.md", "tipo: done\ntask: 80\ncontrolli: 3"), {"op": "reports"}]},
    {"name": "own-defects-that-are-a-number", "from": "aggrega_task raises on a check whose `difetti` is a number",
     "steps": [REPORTS, front("2026-10-01-81-a.md", "tipo: done\ntask: 81\ncontrolli:\n  - tipo: test\n    difetti: 2"),
               consolidate(now="2026-11-26T00:00:00Z", store_name="missing")]},
    {"name": "own-store-text-keys-and-stretches", "from": "the store's lines, keys and fingerprints outside ASCII, and a stretch's key",
     "steps": [REPORTS,
               upsert([rec("d-è", 9, "2026-10-15T10:00:00Z", agente="clàude", sessione="sé😀", token={"in": 2, "out": 3}),
                       {"schema": SCHEMA, "ts_inizio": "2026-10-15T09:00:00Z", "ts_fine": "2026-10-15T09:30:00Z",
                        "progetto": "alpha", "attribuzione": "dichiarata", "task": None, "ruolo": "interattiva",
                        "agente": "claude", "modello": "opus", "effort": None, "sessione": "sé😀", "fonte": "orca-usage-claude",
                        "dispatch": None, "token": None, "esito": None},
                       {"ts_inizio": "2026-09-01T00:00:00Z", "fonte": "x", "sessione": 5, "ts_fine": None},
                       {"ts_inizio": "2026-09-02T00:00:00Z", "fonte": None, "sessione": True, "dispatch": 0, "ts_fine": None},
                       {"ts_inizio": "2026-09-03T00:00:00Z", "dispatch": 7, "ts_fine": None}]),
               read(),
               front("2026-10-16-9-a.md", 'tipo: done\ntask: 9\nsessione: "sé😀"'),
               consolidate(project="alpha"),
               mark(10, [rec("d-è", 10, "2026-06-15T10:00:00Z")], project="àlpha"),
               {"op": "fingerprint", "records": [rec("b", 3, "2026-10-01T00:00:00Z"), rec("a", 3, "2026-10-02T00:00:00Z")]},
               consolidated()]},
    {"name": "own-store-files-and-pruning", "from": "file_mensili, consolidati and pota on the edges",
     "steps": [{"op": "write", "path": "store/2026-1x.jsonl",
                "text": '{"ts_inizio": "2026-11-02T00:00:00Z", "fonte": "f", "sessione": "odd", "ts_fine": null}\n\n'},
               {"op": "write", "path": "store/notes.jsonl", "text": "not read\n"},
               {"op": "write", "path": "store/consolidati.json", "text": "not json"},
               upsert([rec_store("p1", t="2026-05-01T10:00:00Z", task=20), rec_store("p2", t="2026-05-02T10:00:00Z", task=20),
                       rec_store("q1", t="2026-05-03T10:00:00Z", task=21),
                       rec_store(None, t="2026-05-04T10:00:00Z", dispatch=None, fonte="f", sessione="gone", task=None)],
                      now="2026-06-01T00:00:00Z"),
               mark(20, [rec_store("p1", t="2026-05-01T10:00:00Z", task=20), rec_store("p2", t="2026-05-02T10:00:00Z", task=20)]),
               mark(21, [rec_store("q1", t="2026-05-03T10:00:00Z", task=21, esito="failed")]),
               read(),
               prune(now="2026-11-26T00:00:00Z"),
               read(), consolidated(),
               upsert([rec_store("p1", t="2026-05-01T10:00:00Z", task=20), rec_store("p3", t="2026-05-05T10:00:00Z", task=20),
                       rec_store("p3", t="2026-05-05T10:00:00Z", task=20, esito="x"), rec_store("q2", t="2026-11-20T10:00:00Z", task=21)],
                      now="2026-11-26T00:00:00Z", remove=["dispatch:q1", "dispatch:q1", "nothing"]),
               read()]},
    {"name": "own-a-record-without-its-end", "from": "aggrega_task raises when a record has no `ts_fine`",
     "steps": [REPORTS, {"op": "write", "path": "store/2026-10.jsonl",
                         "text": '{"ts_inizio": "2026-10-01T00:00:00Z", "progetto": "alpha", "task": 4, "fonte": "f", "sessione": "s"}\n'},
               consolidate()]},
]


def rel(value, root):
    """Every string under the case's folder, written relative to it."""
    if isinstance(value, dict):
        return {key: rel(item, root) for key, item in value.items()}
    if isinstance(value, list):
        return [rel(item, root) for item in value]
    if isinstance(value, str) and value.startswith(root):
        return "<root>" + value[len(root):].replace("\\", "/")
    return value


def snapshot(root):
    files = {}
    for path in sorted(p for p in root.rglob("*") if p.is_file()):
        name = path.relative_to(root).as_posix()
        text = path.read_bytes().decode("utf-8")
        if path.name == store.CONSOLIDATI:
            text = STAMP.sub('"ts": "<ts>"', text)
        files[name] = text
    return files


def instant(text):
    return datetime.fromisoformat(text)


def execute(step, root, project):
    op = step["op"]
    the_store = lambda: store.Store(root / step["store"])  # noqa: E731
    if op == "mkdir":
        (root / step["path"]).mkdir(parents=True, exist_ok=True)
        return None
    if op in ("write", "report"):
        path = root / step["path"] if op == "write" else project / ".regia" / "report" / step["name"]
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(step["text"].encode("utf-8"))
        return None
    if op == "upsert":
        return the_store().upsert(step["records"], rimuovi=step.get("remove", ()), adesso=instant(step["now"]))
    if op == "mark":
        the_store().segna_consolidato(step["project"], step["task"], step["records"])
        return None
    if op == "prune":
        return the_store().pota(instant(step["now"]))
    if op == "consolidate":
        return consolida.consolida(step["project"], project, the_store(), instant(step["now"]))
    if op == "read":
        return [[key, record] for key, record in the_store().leggi().items()]
    if op == "consolidated":
        return {key: {**entry, "ts": "<ts>"} if isinstance(entry, dict) and "ts" in entry else entry
                for key, entry in the_store().consolidati().items()}
    if op == "front_matter":
        return report.front_matter(step["text"])
    if op == "reports":
        return report.leggi_report(project)
    if op == "fingerprint":
        return store.impronta(step["records"])
    raise ValueError(f"unknown step {op}")


def run(case, root):
    shutil.rmtree(root, ignore_errors=True)
    root.mkdir(parents=True)
    project = root / "project"
    steps = []
    for step in case["steps"]:
        try:
            expected = {"result": rel(execute(step, root, project), str(root))}
        except Exception as error:  # noqa: BLE001 - what the prototype raises is what the product must refuse
            expected = {"error": type(error).__name__}
        expected["files"] = snapshot(root)
        steps.append({**step, "expected": expected})
        if "error" in expected:
            break
    return {"name": case["name"], "from": case["from"], "steps": steps}


def main():
    work = Path(sys.argv[1]).resolve()
    out = {"cases": [run(case, work / case["name"]) for case in CASES]}
    sys.stdout.reconfigure(encoding="utf-8")
    print(json.dumps(out, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
