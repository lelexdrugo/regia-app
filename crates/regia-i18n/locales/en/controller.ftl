### The text of `controller run`: a line a round, as the prototype's summary of a round, and what the resident tells
### its user. A message takes one variable at most: the code joins the parts. The words of the controller's format
### (decisions, outcomes, actions) are data, and stay as they are.

# A round: "<round> <instant>: <precheck> -> <decision>", then in brackets the launches, the operations, the
# wake-up, and the wake from a suspension.
controller-round = round { $round }
controller-precheck = precheck exit { $exit }
controller-no-precheck = precheck not run
controller-operations = [operations: { $list }]
# "[wake-up: <outcome>]": the outcome is "ok" or "failed (<error>)", then ", token renewed".
controller-wake = [wake-up: { $outcome }]
controller-wake-ok = ok
controller-wake-failed = failed ({ $error })
controller-wake-renewed = , token renewed
controller-after-suspension = (wake-up from a suspension)
# "<alarm> <since><stand still> <reason>".
controller-budget-alarm = WARNING: Claude's budget unknown for { $rounds } rounds
controller-budget-since = (since { $since })
controller-budget-stand-still = : { -product } launches nothing until the probe answers.
controller-budget-reason = Reason: { $reason }
# "<round>: error: <error>".
controller-round-failed = round { $round }: error:
controller-wakeup = wake-up after a suspension: a round at once
controller-log-not-written = controller: log not written: { $error }
controller-status-not-written = controller: state file not written: { $error }
# The exits.
controller-stopped = controller: interrupted
# "<another> <holder>".
controller-another = controller: another controller is active
controller-another-holder = ({ $holder })
controller-lock-lost = controller: { $error }
controller-failed = controller: { $error }
controller-bad-option = controller: wrong usage: the interval, the steps of a round, the step's limit and the precheck's timeout must be positive
controller-fixed-clock = { $variable } fixes the clock: it holds for a single round only (--once)
# A shadow run.
controller-shadow = shadow run: a fresh copy of the state in { $folder }, and nothing is launched
controller-shadow-released = in the copy, the controller's lock of { $holder } is released
# The sessions the usage limit stopped (#470): "[resume: <session> (<adapter>), ...]" after the launches, and
# "[suspended until <instant>]" before the operations.
controller-resumes = [resume: { $list }]
controller-suspended-until = [suspended until { $until }]
