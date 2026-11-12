### The text of `budget`: a line per provider, "<Name> [plan]: <parts>", the parts joined by "  |  ",
### and from the cache its notes after two spaces. A message takes one variable at most: the code joins
### the parts.

budget-unknown = sconosciuto ({ $reason })
budget-probe-off = sonde d'uso spente: non hai dato il consenso
# The windows' labels.
budget-window-5h = 5h
budget-window-7d = 7g
# A window is "<label> <used> · <reset> <in>", or "<label> <used> · <not open>".
budget-used = { $pct }% usato
budget-not-open = finestra non aperta (si apre al primo messaggio)
budget-reset = reset { $when }
budget-reset-in = (tra { $span })
# A span is "<days> <hours>", "<hours> <minutes>" or "<minutes>".
budget-days = { $n } g
budget-hours = { $n } h
budget-minutes = { $n } min
budget-token-expired = token scaduto il { $when }: fai girare Codex per rinnovarlo
# "<expiring> <renew>".
budget-token-expiring = token scade tra { $span }
budget-token-renew = (il { $when }): fai girare Codex per rinnovarlo
budget-age = [dato di { $span } fa]
budget-update-failed = [aggiornamento fallito: { $reason }]
