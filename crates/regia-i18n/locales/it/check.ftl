## `check` and `path` (#448): their text. The JSON is the contract; a finding's text and why, and the warnings, are
## the prototype's sentences, which pass as they are.

check-summary = controlli: { $count } voci
check-warning = ({ $warning })
check-thresholds = controlli: { $reason }
# What a document says of a reference that is not on this machine (#443/V9).
reference-absent = non presente su questa macchina
path-reference = riferimento: { $value }
path-kind = tipo: { $value }
path-path = percorso: { $value }
path-unresolved = non si risolve
path-exists = esiste: { $value }
path-exists-file = sì (file)
path-exists-folder = sì (cartella)
path-exists-no = no
path-reason = motivo: { $value }
path-cite = si cita: { $value }
path-no-dossier = errore: la cartella di lavoro non è un dossier (manca .regia/): passa --project
# `step finish`'s line of checks (#701), the prototype's: unavailable, and how many findings the line does not list.
check-unavailable = controlli: non disponibile: { $reason }
check-more = … e altre { $count } (regia controlla --progetto …)
