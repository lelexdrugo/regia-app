### The text of `budget`: a line per provider, "<Name> [plan]: <parts>", the parts joined by "  |  ",
### and from the cache its notes after two spaces. A message takes one variable at most: the code joins
### the parts.

budget-unknown = unknown ({ $reason })
budget-probe-off = usage probes off: you did not consent to them
# The windows' labels.
budget-window-5h = 5h
budget-window-7d = 7d
# A window is "<label> <used> · <reset> <in>", or "<label> <used> · <not open>".
budget-used = { $pct }% used
budget-not-open = window not open (it opens at the first message)
budget-reset = resets { $when }
budget-reset-in = (in { $span })
# A span is "<days> <hours>", "<hours> <minutes>" or "<minutes>".
budget-days = { $n } d
budget-hours = { $n } h
budget-minutes = { $n } min
budget-token-expired = token expired on { $when }: run Codex to renew it
# "<expiring> <renew>".
budget-token-expiring = token expires in { $span }
budget-token-renew = (on { $when }): run Codex to renew it
budget-age = [data from { $span } ago]
budget-update-failed = [update failed: { $reason }]
