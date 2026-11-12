### `comment` and `declare`: what they print, in the prototype's words. A message takes one variable at
### most: the code joins the parts of a line.

## `comment`

comment-added = comment added to #{ $number }
# On stderr, followed by why in parentheses: the labels or the state could not be read. The comment exists.
comment-id-not-recorded = warning: the id of comment { $id } was not recorded in the step

## `declare`. The line is `declared: <project>`, then ` #<task>` with a task, then ` (<session>)`.

declare-done = declared: { $project }
declare-session = (session { $session })

## The record of a command in the sessions' log, which fails silently: this line only with the debug
## variable set, followed by why in parentheses.

activity-not-recorded = debug: activity not recorded
