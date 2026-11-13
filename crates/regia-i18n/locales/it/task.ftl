### The writes of the plan (`task status` and the other commands that change a task): what they print, in
### the prototype's words. A message takes one variable at most: the code joins the parts of a line, which
### starts with `#<number>`.

## `task status`: `#<number> → <status>`, the status in the words of `state-*`.

task-status-set = → { $status }

## `task unblock`: `#<number> sbloccato da #3, #6`, or `#<number>: nessun bloccante da togliere`. The warning
## is followed by ` #<number>`. The comment on the issue is `<task-unblock-comment>: <reason>`, then signed.

task-unblocked = sbloccato da { $blockers }
task-unblock-none = nessun bloccante da togliere
task-not-a-blocker = avviso: #{ $blocker } non era fra i bloccanti di
task-unblock-comment = Sbloccato da { $blockers }

## `task new`: `task #<n> creato (padre #<p>, stato <status>, <maturity>) <url>`.

task-created = task #{ $number } creato
task-created-parent = padre #{ $parent }
task-created-status = stato { $status }
maturity-draft = abbozzo
maturity-ready = pronto

## On stderr, for `task new` and `task move`: this engine does not write the Project's field that mirrors the
## project label (#454 of the dossier). The command does its work all the same.

task-project-field-not-written = avviso: campo «Progetto» non scritto da questo motore; l'etichetta { $label } resta la fonte, `regia progetto riempi` lo riallinea

## `task maturity`: `#<number> → <label>`. `task block`: `#<number> bloccato da <list as given>`. `task parent`:
## `#<number> è già sub-issue di #<parent>` or `#<number> ora è sub-issue di #<parent>`. `task move`:
## `#<number>: progetto <from> → <to>, sub-issue di #<parent>`.

task-maturity-set = → { $label }
task-blocked = bloccato da { $by }
task-parent-already = è già sub-issue di #{ $parent }
task-parent-set = ora è sub-issue di #{ $parent }
task-moved-from = progetto { $project }
task-moved-parent = sub-issue di #{ $parent }
