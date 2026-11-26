# The cases of `consolidate`, the raw store and the reports (#491)

`oracle.json` holds 36 cases, recorded on the prototype by `crates/regia-conformance/oracle/consolidate_oracle.py`
and replayed on the product, step by step, by `crates/regia-conformance/tests/moviola_consolidate.rs`: one test
each, named as the case with `_` for `-`. Every step must give the prototype's result, fail where the prototype
raised, and leave the same bytes in every file of the case. Only the instants that `segna_consolidato` takes from
its clock (`ts` in `consolidati.json`) are written `<ts>` on both sides. The data is synthetic: the projects are
`alpha`, `beta` and two with a letter outside ASCII, the sessions and the folders are invented.

The prototype's tests are named as in the dossier: `tools/tests/test_moviola.py` and `tools/tests/test_moviola_viewer.py`.

## The prototype's tests

| Prototype's test | Case |
|---|---|
| `TestReport.test_front_matter_completo` | `report-front-matter-complete` |
| `TestReport.test_difetti_chiusi_a_blocchi_e_report_senza_campi` | `report-closed-defects-in-a-block-and-no-fields` |
| `TestReport.test_leggi_report_distingue_assente_da_zero` | `report-absent-is-not-zero` |
| `TestReport.test_leggi_report_espone_stop_reason_e_senza_il_campo_resta_none` | `report-stop-reason` |
| `TestStore.test_upsert_idempotente` | `store-upsert-is-idempotent` |
| `TestStore.test_un_dispatch_in_corso_si_sostituisce` | `store-a-running-dispatch-is-replaced` |
| `TestStore.test_rimuovi_sessione_assorbita` | `store-an-absorbed-session-is-removed` |
| `TestStore.test_sync_due_volte_stesso_numero_di_record` | `store-the-same-records-twice`: the records the prototype's Orca adapter gives on the test's synthetic Orca files, written into the case, since the adapter (`sync`) is not ported |
| `TestStore.test_lo_store_non_puo_stare_in_temp` | not a case: `crates/regia-conformance/tests/moviola_store_folder.rs`, `every_rule_refuses_the_temporary_folder_and_none_creates_the_folder`, where the store's rule meets `regia-store`'s refusal |
| `TestStore.test_ritenzione_non_riammette_ne_elimina_senza_consolidamento` | `store-retention-and-consolidation` |
| `TestStore.test_un_record_nuovo_dopo_il_consolidamento_blocca_la_potatura` | `store-a-late-record-holds-the-pruning` |
| `TestConsolida.test_aggregato_con_e_senza_campi_di_processo` | `consolidate-with-and-without-process-fields` |
| `TestConsolida.test_task_con_solo_il_report_ha_la_sua_riga` | `consolidate-a-task-with-reports-only` |
| `TestConsolida.test_revisione_fatta_in_sessione_conta_i_difetti_chiusi_da_un_altro_task` | `consolidate-a-review-in-session-counts-what-another-task-closed` |
| `TestConsolida.test_solo_report_non_aggira_il_rinvio_di_un_task_in_corso` | `consolidate-reports-only-does-not-bypass-a-running-task` |
| `TestConsolida.test_riga_nata_dal_grezzo_scaduto_non_diventa_solo_report` | `consolidate-a-row-of-expired-records-stays` |
| `TestConsolida.test_difetti_chiude_nel_task_di_correzione` | `consolidate-closes-on-the-fixing-task` |
| `TestConsolida.test_token_di_cache_si_sommano_solo_dove_ci_sono` | `consolidate-cache-tokens-only-where-they-are` |
| `TestConsolida.test_round_senza_report_conta_i_dispatch` | `consolidate-round-counts-the-dispatches` |
| `TestConsolida.test_task_con_dispatch_in_corso_si_rinvia` | `consolidate-a-running-task-is-deferred` |
| `TestConsolida.test_riesecuzione_stesso_file_e_righe_di_task_scaduti_si_conservano` | `consolidate-again-and-expired-rows-are-kept` |
| `TestConsolida.test_aggregati_e_store_senza_contenuti_riservati` | `consolidate-nothing-reserved`: the adapter's records, as above; that no title, spec or archive reaches them is the adapter's property, and the case shows `consolidate` adds none |
| `TestSessioneCitata.test_la_chat_dell_orchestratore_va_ai_task_che_la_citano` | `cited-the-orchestrator-s-chat-goes-to-the-tasks-that-name-it` |
| `TestSessioneCitata.test_dispatch_del_task_piu_sessione_citata` | `cited-a-dispatch-of-the-task-and-a-named-session` |
| `TestSessioneCitata.test_un_dispatch_senza_task_si_cita_con_il_suo_id` | `cited-a-dispatch-without-a-task-by-its-id` |
| `TestSessioneCitata.test_il_legame_sopravvive_alla_scadenza_e_la_riga_resta_aggiornabile` | `cited-the-link-outlives-the-store-and-the-row-stays-open` |
| `TestCli.test_sync_e_consolida_da_riga_di_comando` | not ported: the command line is the next phase, after the gate of its name |
| `TestCli.test_progetto_sconosciuto` | not ported, as above |

The 15 cases of `consolida` are the 11 of `TestConsolida` and the 4 of `TestSessioneCitata`; the 7 of `store` and
the 4 of `report` are those of `TestStore` and `TestReport`.

## The cases of this port

| Case | What it tries |
|---|---|
| `report-the-four-models-of-the-contract` | one report for each model of the worker's report in `contratti.md` (`done`, `gate`, `proposal`, `failed`), read alone and in a folder, then consolidated |
| `own-no-store-reports-only` | Acceptance 2 of #491: no raw store, no error, the rows from the reports alone, and no store made |
| `own-no-dossier-folder` | a dossier's folder that is not there: the empty aggregates are written, and the folders made |
| `own-rows-as-written` | the rows already written that Python's `int()` keeps (a task `"5"`, `2.9`, `true`, `" 6 "`) and those it drops, their keys in their order |
| `own-a-row-from-reports-alone-keeps-its-cited-entries` | a row with only `via: report` entries, which the reports update and whose old entries outlive the store |
| `own-reports-the-edges-of-the-subset` | the reader's edges: a byte order mark, CRLF, comments, both quotes, inline and block lists, a key read twice, a digit outside ASCII, a task from the file's name, a folder named as a report |
| `own-a-check-that-is-a-number` | `controlli: 3`, on which the prototype raises |
| `own-defects-that-are-a-number` | a check whose `difetti` is a number, on which the prototype raises |
| `own-store-text-keys-and-stretches` | the store's lines and fingerprints outside ASCII, a stretch's key, keys from `None`, `True` and numbers, the record of what was consolidated with its keys escaped |
| `own-store-files-and-pruning` | the monthly files the store reads (`????-??.jsonl`), a record of what was consolidated that is not JSON, a fingerprint that holds and one that does not, keys read twice and removed twice |
| `own-a-record-without-its-end` | a record with no `ts_fine`, on which the prototype raises |

## The functions of the prototype's modules, and the tests that cover them

| Python | Rust (`regia_core::moviola`) | Covered by |
|---|---|---|
| `report._senza_commento` | `report::without_comment` | `own-reports-the-edges-of-the-subset`, `report-front-matter-complete`; unit tests |
| `report._scalare` | `report::scalar` | the same; unit tests |
| `report._lista_inline` | `report::inline_list` | the same; unit tests |
| `report._valore` | `report::value` | the same |
| `report._blocco` | `report::block` | `report-closed-defects-in-a-block-and-no-fields`, `own-reports-the-edges-of-the-subset` |
| `report.front_matter` | `front_matter` | every `report-*` case |
| `report._intero` | `report::whole` | `own-reports-the-edges-of-the-subset`, `report-absent-is-not-zero` |
| `report.leggi_report` | `read_reports` | `report-absent-is-not-zero`, `report-stop-reason`, `report-the-four-models-of-the-contract`, `own-*` |
| `store.cartella_store`, `store._rifiuta_temp` | `folder_rule`, with `regia_identity::state_dir` and `regia_store::is_within` | `moviola_store_folder.rs` |
| `store.chiave` | `key` | every `read` step; `own-store-text-keys-and-stretches` |
| `store.richiede_consolidamento` | `needs_consolidation` | every `consolidate` and `prune` step |
| `store.chiave_task` | `task_key` | `store-retention-and-consolidation`, `own-store-files-and-pruning` |
| `store.riga` | `line` | the bytes of every monthly file and aggregate |
| `store.impronta` | `fingerprint` | the bytes of every `consolidati.json`; the `fingerprint` step |
| `store._scrivi_atomico` | `store::write_atomic` | every case that writes |
| `store.Store.file_mensili` | `ActivityStore::month_files` | `own-store-files-and-pruning` |
| `store.Store.leggi` | `ActivityStore::read` | every `read` step |
| `store.Store.consolidati` | `ActivityStore::consolidated` | every `consolidated` step; `own-store-files-and-pruning` |
| `store.Store.segna_consolidato` | `ActivityStore::mark_consolidated` | every `mark` and `consolidate` step |
| `store.Store._scrivi_mesi`, `store.Store._per_mese` | `ActivityStore::write_months` | every `upsert` and `prune` step |
| `store.Store.upsert` (with `rimuovi`) | `ActivityStore::upsert` | every `store-*` case |
| `store.Store.upsert`: `sostituisci`, `assorbite` | not ported: only `sync` passes them | — |
| `store.Store.pota` | `ActivityStore::prune` | `store-retention-and-consolidation`, `store-a-late-record-holds-the-pruning`, `own-store-files-and-pruning`, every `consolidate` step |
| `store._taglio` | `store::cut` | the same; unit test |
| `consolida._riferimenti_chiusi` | `consolidate::closed_by_task` | `consolidate-with-and-without-process-fields`, `consolidate-closes-on-the-fixing-task` |
| `consolida._somma_token` | `consolidate::sum_tokens` | `consolidate-cache-tokens-only-where-they-are`, `consolidate-with-and-without-process-fields` |
| `consolida._chiude` | `consolidate::closes` | `consolidate-closes-on-the-fixing-task`, `consolidate-a-review-in-session-counts-what-another-task-closed` |
| `consolida.sessioni_citate` | `consolidate::cited` | the `cited-*` cases |
| `consolida._voci_citate` | `consolidate::cited_entries` | the `cited-*` cases |
| `consolida._con_citate_precedenti` | `consolidate::with_earlier_citations` | `cited-the-link-outlives-the-store-and-the-row-stays-open`, `own-a-row-from-reports-alone-keeps-its-cited-entries` |
| `consolida.aggrega_solo_report` | `consolidate::aggregate_reports_only` | `consolidate-a-task-with-reports-only`, `own-no-store-reports-only` |
| `consolida.aggrega_task` | `consolidate::aggregate_task` | every `consolidate-*` case |
| `consolida.percorso_aggregati` | `aggregates_file` | every `consolidate` step (`file`) |
| `consolida._leggi_righe` | `consolidate::read_rows` | `own-rows-as-written`, `consolidate-again-and-expired-rows-are-kept` |
| `consolida.consolida` | `consolidate` | every `consolidate` step; `without_a_store_nothing_fails_and_no_store_is_made` |
