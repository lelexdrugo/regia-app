## `check` and `path` (#448): their text. The JSON is the contract; a finding's text and why, and the warnings, are
## the prototype's sentences, which pass as they are.

check-summary = checks: { $count } findings
check-warning = ({ $warning })
check-thresholds = checks: { $reason }
# What a document says of a reference that is not on this machine (#443/V9).
reference-absent = not present on this machine
path-reference = reference: { $value }
path-kind = kind: { $value }
path-path = path: { $value }
path-unresolved = does not resolve
path-exists = exists: { $value }
path-exists-file = yes (file)
path-exists-folder = yes (folder)
path-exists-no = no
path-reason = reason: { $value }
path-cite = cited as: { $value }
path-no-dossier = error: the working folder is not a dossier (no .regia/): pass --project
# `step finish`'s line of checks (#701): unavailable, and how many findings the line does not list.
check-unavailable = checks: unavailable: { $reason }
check-more = … and { $count } more (regia check --project …)
