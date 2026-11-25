## `setup`, `project new` and `project set` (#452): the prototype's words, which the Italian side prints as it does.

## `setup`: the one line it prints.

setup-labels-ready = base labels ready

## `project new`: `progetto <id>: obiettivo #<number>`, a line `  creato <path>` for each file of the scaffold, and with
## `--repo` `  repo privato <repo>, cartella <inizializzata | già un repo>, origin <url>` and `  primo push: <outcome>`.

project-new-project = project { $project }:
project-new-objective = objective #{ $number }
project-new-created = created { $path }
project-new-repo = private repo { $repo },
project-new-folder-init = folder initialized,
project-new-folder-existing = folder already a repo,
project-new-origin = origin { $url }
project-new-first-push = first push: { $outcome }

## The outcome of the first push, a sentence the JSON carries too (`first_push`), and the first commit's message.

project-first-push-pushed = pushed { $sha }
project-first-push-nothing = nothing to commit
project-first-push-no-branch = no commit made: { $reason }
project-first-push-add-failed = git add failed: { $reason }
project-first-push-commit-failed = git commit failed: { $reason }
project-first-push-push-failed = local commit made, git push failed: { $reason }
project-first-commit = { $project }: start of the project

## The Objective's sections, after its registry, and its mandate when none is given.

project-objective-heading = Objective
project-definition-of-done-heading = Definition of done
project-mandate-heading = Mandate
project-mandate-to-come = To be defined at the first proposal.

## Warnings on stderr: the project exists, and what follows did not happen.

project-new-field-not-written = warning: the Project's field «Progetto» is not written by this engine for the project { $project }; the label stays the source, and `regia project mirror` realigns it
project-new-topic-not-added = warning: topic not added to the repo { $repo }

## `project set`: `progetto <id> (#<number>): repo=… prodotti=… wip=… priorita=…`, `(dry-run, nulla scritto)` on a
## dry run, and `  leve cambiate: <lever> <before> → <after>, …` when a lever changed. The comment on the Objective
## is `Leve: <lever> <before> → <after>, …`.

project-set-project = project { $project }
project-set-repo = repo={ $value }
project-set-products = products={ $value }
project-set-wip = wip={ $value }
project-set-priority = priority={ $value }
project-set-dry-run = (dry run, nothing written)
project-set-levers-changed = levers changed: { $changes }
project-levers-comment = Levers: { $changes }
project-levers-comment-failed = warning: levers written, trace comment not published
