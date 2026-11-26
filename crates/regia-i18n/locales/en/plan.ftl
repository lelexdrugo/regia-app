### `plan` (#444): the tree of the open tasks, the line before the diagram, the warnings and the errors. The JSON is
### the contract, and the diagram in it is data in the prototype's words (`formats/plan_v1.toml` of `regia-core`):
### these are the words around it. Separators, brackets and arrows stay in the code, since a message loses its blanks.

plan-nothing = (no task)
plan-outside = one step outside the perimeter:
plan-cycle = in a cycle
plan-unblocks = unblocks { $unblocks }
plan-unblocks-total = ({ $total } with Inbox and Proposals)
plan-chain = chain { $chain }
plan-waits = waits for { $numbers }
plan-note-inbox = { $count } Inbox tasks without edges:
plan-note-nested = edges between a parent and its child, not drawn:
plan-note-truncated = { $count } open tasks cut over the cap
plan-note-lost = { $lost } edges lost:
plan-note-hint = for the rest use { $option } on a subtree
plan-main-line = main line: { $line }
plan-markdown-head = Plan from the tracker on { $date }
plan-markdown-legend = ✋ waits for the owner, thick arrows = main line
plan-children-waiting = Children that wait for their parent with an edge that would close a cycle, and does not count: { $edges }
plan-max-nodes = { $option } must be at least 1
plan-root-missing = issue #{ $number } not found in the tracker
plan-project-missing = project «{ $project }» has no task in the tracker
