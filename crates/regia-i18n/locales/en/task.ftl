### The writes of the plan (`task status` and the other commands that change a task): what they print, in
### the prototype's words. A message takes one variable at most: the code joins the parts of a line, which
### starts with `#<number>`.

## `task status`: `#<number> → <status>`, the status in the words of `state-*`.

task-status-set = → { $status }

## `task unblock`: `#<number> sbloccato da #3, #6`, or `#<number>: nessun bloccante da togliere`. The warning
## is followed by ` #<number>`. The comment on the issue is `<task-unblock-comment>: <reason>`, then signed.

task-unblocked = unblocked from { $blockers }
task-unblock-none = no blocker to remove
task-not-a-blocker = warning: #{ $blocker } was not among the blockers of
task-unblock-comment = Unblocked from { $blockers }

## `task new`: `task #<n> creato (padre #<p>, stato <status>, <maturity>) <url>`.

task-created = task #{ $number } created
task-created-parent = parent #{ $parent }
task-created-status = status { $status }
maturity-draft = draft
maturity-ready = ready

## On stderr, for `task new` and `task move`: this engine does not write the Project's field that mirrors the
## project label (#454 of the dossier). The command does its work all the same.

task-project-field-not-written = warning: the Project's field «Progetto» is not written by this engine; the label { $label } stays the source, and `regia project mirror` realigns it

## `task maturity`: `#<number> → <label>`. `task block`: `#<number> bloccato da <list as given>`. `task parent`:
## `#<number> è già sub-issue di #<parent>` or `#<number> ora è sub-issue di #<parent>`. `task move`:
## `#<number>: progetto <from> → <to>, sub-issue di #<parent>`.

task-maturity-set = → { $label }
task-blocked = blocked by { $by }
task-parent-already = is already a sub-issue of #{ $parent }
task-parent-set = is now a sub-issue of #{ $parent }
task-moved-from = project { $project }
task-moved-parent = sub-issue of #{ $parent }
