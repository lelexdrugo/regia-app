### The budget policy: `policy show`, `set`, `reset` and `proposals`. A message takes one variable at
### most: the code joins the parts of a line, and puts "- " before each item of the policy.

## Sources of the values, in the title.

policy-source-file = file
policy-source-file-local = file+local
policy-source-default = default
policy-source-default-local = default+local

## The policy in use.

policy-title = Budget policy in use (source: { $source }):
policy-automatic = automatic work: below { $pct }% of Claude's 5-hour window
policy-backfill = backfill (steps started only by new ideas in Inbox): below { $pct }%
policy-backfill-off = backfill: no threshold of its own, the automatic work's applies
policy-drain-on = backlog to drain (Inbox and drafts): on
policy-drain-off = backlog to drain (Inbox and drafts): off ({ $file })
# `$caps` is the two parts below, joined by ", ".
policy-caps = Proposal caps: { $caps }
policy-caps-open = { $n } open per project
policy-caps-new = { $n } new in 7 days
policy-caps-off = Proposal caps: off
policy-local-none = local override: none
# `$when` is local time, to the minute. A note follows, after " · ".
policy-local-until = local override: active until { $when }
policy-local-unbounded = local override: active until it is reset
# `$detail` is the reason, or the expiry.
policy-local-expired = local override: expired ({ $detail })
policy-local-unreadable = local override: unreadable ({ $detail })
policy-local-ignored = local override: ignored ({ $detail })
policy-defaults = Warning: { $file } does not read, the code's defaults apply.

## `policy set` and `policy reset`.

policy-not-written = override not written: { $reason }
policy-removed = Local override removed.
policy-nothing-to-remove = No local override to remove.

## `policy proposals`. The line is "<project>: <open>; <new>." and a count is "<n> <out of>".

policy-proposals-open = open Proposals { $count }
policy-proposals-new = new in the last 7 days { $count }
policy-out-of = of { $cap }
policy-new-unknown = not available
policy-room-caps-off = The Proposal caps are off (local override): there is room.
policy-room-left = There is room for a new Proposal.
policy-room-none = No room left: the idea stays in Inbox (protocol §1.2 point 6).
policy-unknown-project = unknown project: { $project }
policy-weekly-unavailable = weekly count not available: { $reason }
