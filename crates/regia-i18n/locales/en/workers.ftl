### `workers` (#744): the text of the workers of Orca on the tasks asked, a line for each of the prototype's. The JSON
### is the contract. A message takes one variable at most: the code joins the parts of a line with "; ", and a block's
### case is the contract's word (`open-call`, `waiting-for-person`) in every language.

## A task's line: "#<task>: " and one of these.

workers-none = no worker of Orca in progress
workers-unreadable = the worker of Orca does not read right now (which does not mean there is none)
workers-last-line = last line { $minutes } min ago
workers-waiting = waits for a person
workers-not-waiting = does not wait for a person
workers-past-threshold = past the threshold of { $minutes } min
workers-within-threshold = within the threshold of { $minutes } min
# `$detail` is the case and `workers-past`, joined by ", ".
workers-blocked = BLOCKED ({ $detail })
workers-past = past { $minutes } min
workers-cause-limit = STOPPED BY THE SESSION LIMIT (it passes by itself: the task back to Approved, protocol §1.3)
workers-cause-billing = STOPPED BY BILLING (only the owner fixes it: the task back to Approved and the cross-cutting Decision, protocol §1.3)
workers-only-silent = past { $minutes } min but only silent (no open call, no wait): not blocked
# `$activity` is Orca's own word.
workers-activity = (Orca: activity { $activity })

## More than one dispatch on a task: the count after the task's line, then "    Dispatch <n>/<count>: " and a line each.

workers-in-progress = { $count } Dispatches in progress on the same task
workers-blocked-count = { $count } BLOCKED
workers-dispatch = Dispatch { $position }
workers-dispatch-unreadable = does not read right now

## stderr, with exit 1.

workers-unreachable = error: Orca is not reachable ({ $reason })
