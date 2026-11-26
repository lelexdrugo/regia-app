### `workers` (#744): the text of the workers of Orca on the tasks asked, a line for each of the prototype's. The JSON
### is the contract. A message takes one variable at most: the code joins the parts of a line with "; ", and a block's
### case is the contract's word (`open-call`, `waiting-for-person`) in every language.

## A task's line: "#<task>: " and one of these.

workers-none = nessun worker di Orca in corso
workers-unreadable = worker di Orca non leggibile in questo momento (non vuol dire che non ci sia)
workers-last-line = ultima riga { $minutes } min fa
workers-waiting = aspetta una persona
workers-not-waiting = non aspetta una persona
workers-past-threshold = oltre la soglia di { $minutes } min
workers-within-threshold = entro la soglia di { $minutes } min
# `$detail` is the case and `workers-past`, joined by ", ".
workers-blocked = BLOCCATO ({ $detail })
workers-past = oltre { $minutes } min
workers-cause-limit = FERMO PER LIMITE DI SESSIONE (passa da solo: task di nuovo Approvato, protocollo §1.3)
workers-cause-billing = FERMO PER FATTURAZIONE (la sistema solo il titolare: task di nuovo Approvato e Decisione trasversale, protocollo §1.3)
workers-only-silent = oltre { $minutes } min ma solo silente (nessuna chiamata aperta, nessuna attesa): non è bloccato
# `$activity` is Orca's own word.
workers-activity = (Orca: activity { $activity })

## More than one dispatch on a task: the count after the task's line, then "    Dispatch <n>/<count>: " and a line each.

workers-in-progress = { $count } Dispatch in corso sullo stesso task
workers-blocked-count = { $count } BLOCCATO
workers-dispatch = Dispatch { $position }
workers-dispatch-unreadable = non leggibile in questo momento

## stderr, with exit 1.

workers-unreachable = errore: Orca non raggiungibile ({ $reason })
