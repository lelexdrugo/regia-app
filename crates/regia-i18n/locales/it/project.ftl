## `setup`, `project new` and `project set` (#452): the prototype's words, which the Italian side prints as it does.

## `setup`: the one line it prints.

setup-labels-ready = etichette di base pronte

## `project new`: `progetto <id>: obiettivo #<number>`, a line `  creato <path>` for each file of the scaffold, and with
## `--repo` `  repo privato <repo>, cartella <inizializzata | già un repo>, origin <url>` and `  primo push: <outcome>`.

project-new-project = progetto { $project }:
project-new-objective = obiettivo #{ $number }
project-new-created = creato { $path }
project-new-repo = repo privato { $repo },
project-new-folder-init = cartella inizializzata,
project-new-folder-existing = cartella già un repo,
project-new-origin = origin { $url }
project-new-first-push = primo push: { $outcome }

## The outcome of the first push, a sentence the JSON carries too (`first_push`), and the first commit's message.

project-first-push-pushed = pushato { $sha }
project-first-push-nothing = niente da committare
project-first-push-no-branch = commit non fatto: { $reason }
project-first-push-add-failed = git add non riuscito: { $reason }
project-first-push-commit-failed = git commit non riuscito: { $reason }
project-first-push-push-failed = commit locale fatto, git push non riuscito: { $reason }
project-first-commit = { $project }: avvio del progetto

## The Objective's sections, after its registry, and its mandate when none is given.

project-objective-heading = Obiettivo
project-definition-of-done-heading = Definizione di fatto
project-mandate-heading = Mandato
project-mandate-to-come = Da definire alla prima proposta.

## Warnings on stderr: the project exists, and what follows did not happen.

project-new-field-not-written = avviso: campo «Progetto» non scritto da questo motore per il progetto { $project }; l'etichetta resta la fonte, `regia progetto riempi` lo riallinea
project-new-topic-not-added = avviso: topic non aggiunto al repo { $repo }

## `project set`: `progetto <id> (#<number>): repo=… prodotti=… wip=… priorita=…`, `(dry-run, nulla scritto)` on a
## dry run, and `  leve cambiate: <lever> <before> → <after>, …` when a lever changed. The comment on the Objective
## is `Leve: <lever> <before> → <after>, …`.

project-set-project = progetto { $project }
project-set-repo = repo={ $value }
project-set-products = prodotti={ $value }
project-set-wip = wip={ $value }
project-set-priority = priorita={ $value }
project-set-dry-run = (dry-run, nulla scritto)
project-set-levers-changed = leve cambiate: { $changes }
project-levers-comment = Leve: { $changes }
project-levers-comment-failed = avviso: leve scritte, commento di traccia non pubblicato
