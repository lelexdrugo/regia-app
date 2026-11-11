### Words every screen uses: states, actions, errors, toasts and dates.

state-inbox = Inbox
state-proposal = Proposal
state-approved = Approved
state-in_progress = In progress
state-decision = Decision
state-done = Done
state-discarded = Discarded
state-session = Session
action-copy = Copy
loading = Loading
unit-minutes = minutes
error-block = Couldn't read { $what }. Last good data: { $when }.
error-retry = Retry
error-retry-done = Trying again
error-what-brief = the tracker
error-what-daily = the project facts
error-what-moviola = the milestones
error-what-console = the step log
error-brief-sub = GitHub isn't answering: the decisions stay where they are, none has been lost.
error-brief-detail = When the tracker answers again, the first memo opens here.
toast-decided = #{ $n }: { $outcome ->
        [approve] approved
        [discard] discarded
       *[option] chose { $choice }
    }.
toast-seconds = Writing in { $s } s
toast-undone = #{ $n } is back in the queue. Nothing was written.
toast-undone-previous = #{ $n }: stays decided { $choice }.
toast-undo-none = Nothing to undo. Decisions already written can be changed from "Done today".
toast-deferred = #{ $n } deferred to the next dailies.
toast-undo = Undo
toast-copied = Copied
toast-orca = Orca opened the session.
toast-stopped = Agent stopped.
toast-controller-on = Controller on.
toast-controller-off = Controller stopped. Agents already started finish their work.

## Dates: one format for each use, the same on every screen (#439/D30). `$when` is a date. Only the
## webview formats these: Fluent in Rust has no DATETIME.

date-day = { DATETIME($when, day: "numeric", month: "short") }
date-time = { DATETIME($when, hour: "2-digit", minute: "2-digit") }
date-day-time = { DATETIME($when, day: "numeric", month: "short", hour: "2-digit", minute: "2-digit") }
