### `comment` and `declare`: what they print, in the prototype's words. A message takes one variable at
### most: the code joins the parts of a line.

## `comment`

comment-added = commento aggiunto a #{ $number }
# On stderr, followed by why in parentheses: the labels or the state could not be read. The comment exists.
comment-id-not-recorded = avviso: id del commento { $id } non registrato nel passo

## `declare`. The line is `declared: <project>`, then ` #<task>` with a task, then ` (<session>)`.

declare-done = dichiarato: { $project }
declare-session = (sessione { $session })
