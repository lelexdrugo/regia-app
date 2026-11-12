### The writes of the plan (`task status` and the other commands that change a task): what they print, in
### the prototype's words. A message takes one variable at most: the code joins the parts of a line, which
### starts with `#<number>`.

## `task status`: `#<number> → <status>`, the status in the words of `state-*`.

task-status-set = → { $status }
