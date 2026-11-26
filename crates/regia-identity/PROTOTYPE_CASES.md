# The prototype's state-folder cases, and the test here that answers each

The rule of `state_dir` against the dossier's `tools/tests/test_config_locale.py` (class `CartellaDiRuntime`)
and `tools/tests/test_lettori_config_locale.py` (the runtime-folder cases of the moviola store). The Python
names are Italian, so they live here and not in the sources: a guard in `regia-conformance`
(`no_italian_identifiers_in_the_sources`) keeps Italian words out of every `.rs` file.

The prototype stubs the platform and the environment; so does `state_dir_by`, the rule with both given, which
`state_dir` calls with the real ones. Each test below runs both platforms on any machine.

| Id | Prototype case | Test here |
|----|----------------|-----------|
| P1 | `test_regia_runtime_vince_su_tutto` (both platforms) | `the_runtime_variable_wins_over_everything` |
| P2 | `REGIA_RUNTIME vuota vale come non impostata` (`test_lettori_config_locale.py`) | `an_empty_runtime_variable_counts_as_unset` |
| P3 | `test_windows_localappdata`; `solo LOCALAPPDATA`; `XDG_STATE_HOME non conta su Windows` | `on_windows_the_folder_is_under_localappdata_and_xdg_does_not_count` |
| P4 | `test_windows_senza_localappdata_usa_appdata_local_della_home`; `senza LOCALAPPDATA` and `LOCALAPPDATA vuota: sotto il profilo` | `on_windows_without_localappdata_there_is_no_folder` (a **difference**, pinned: the prototype falls back to `<home>/AppData/Local/regia`, the product gives no folder, as the prototype's `radice()` does) |
| P5 | `test_posix_xdg_state_home`; `solo XDG_STATE_HOME`; `LOCALAPPDATA non conta fuori da Windows` | `off_windows_the_folder_is_under_xdg_state_home_and_localappdata_does_not_count` |
| P6 | `test_posix_senza_xdg_usa_local_state_della_home`; `XDG_STATE_HOME vuota`; `nessuna variabile` | `off_windows_without_xdg_state_home_the_folder_is_under_the_home` |
| P7 | `test_non_crea_la_cartella` | `the_rule_does_not_create_the_folder` |
| P8 | `test_rifiuta_la_cartella_temporanea_da_qualunque_regola` (`REGIA_RUNTIME` and `LOCALAPPDATA` under the temporary folder) | not in this crate: the refusal is `regia-store`'s, so `crates/regia-conformance/tests/runtime_folder.rs` tries it end to end (`regia_runtime_under_the_temporary_folder_is_refused`, `the_platform_default_under_the_temporary_folder_is_refused`) |

The `MOVIOLA_ATTIVITA` cases of `test_lettori_config_locale.py` (class `CartellaDelloStore`), where the moviola
store's folder meets this rule, are in `crates/regia-conformance/tests/moviola_store_folder.rs` (#491).
