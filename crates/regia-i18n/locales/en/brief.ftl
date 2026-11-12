### The brief: what waits for the owner, what is ready, what runs.

## Shared with the CLI, which prints the same brief.

brief-waiting = Waiting on you (proposal, decision)
brief-session = Needs a working session with you
brief-ready = Ready to run
brief-in-progress = In progress
brief-by-project = By project
brief-nothing = (nothing)
brief-loading = Loading the brief…
brief-error = The brief could not be read: { $reason }
brief-partial = Not ported yet: controller, dossier and outside-tracker warnings are not shown.
brief-cycle = Dependencies in a cycle, no graph order: { $reason }
brief-truncated = Dependencies only partly read: { $reason }

## The app's own.

brief-title = Brief
brief-finding = { $n ->
        [0] Nothing is waiting for you. The agents carry on by themselves.
        [one] { $n } item is waiting for you. Start with #{ $top }: it unblocks { $unlocks } tasks.
       *[other] { $n } items are waiting for you. Start with #{ $top }: it unblocks { $unlocks } tasks.
    }
brief-updated = Tracker read at { $when }
brief-to-daily = Do the dailies
brief-ready-empty = No task is ready.
brief-running-empty = No agent at work.
brief-dossiers = Dossiers to fix
brief-offtracker = Off the tracker
brief-offtracker-more = … and { $n } more
brief-offtracker-note = Signals with known false positives, not decisions.
brief-range = { $from }–{ $to } of { $total }
brief-more = Show more
brief-empty = No decision is pending.
brief-empty-done = You've answered them all.
brief-empty-sessions = Only the items asking for a session with you remain.
brief-done = Done today
brief-done-choice = { $outcome ->
        [approve] approved
        [discard] discarded
        [defer] deferred
       *[option] chose { $choice }
    }
brief-done-writing = writing
brief-done-written = written
brief-stopped-link = See the console
queue-chain-title = Unblocks { $unlocks } tasks right away; the chain of work waiting is { $chain } long
brief-empty-title = The queue is empty
brief-empty-body = Agents at work: { $running }. Tasks ready: { $ready }. When a choice of yours is needed, it appears here with a recommendation.
brief-empty-console = Open the console
brief-empty-moviola = Open the moviola
queue-unlocks = unblocks { $n }
queue-chain = chain { $n }
dossier-unpushed = { $n ->
        [one] { $n } commit not pushed
       *[other] { $n } commits not pushed
    }
dossier-noremote = no remote
dossier-missing = folder missing

## The app's queue, its blocks and their states. What others wrote (titles, projects, reasons, folders) is
## shown next to these words, never inside them.

brief-error-lead = The brief could not be read:
brief-not-read = Not read: the brief could not be read.
queue-due = due { $day }
queue-overdue = overdue since { $day }
queue-unlocks-total = unblocks { $n } ({ $total } with Inbox and Proposals)
queue-cycle = in a cycle
brief-due-ignored = #{ $n }: the due date is not a date YYYY-MM-DD, so it is ignored:
brief-not-ported = { $section ->
        [controller] This version does not read the controller's warnings yet.
        [dossier] This version does not check the dossiers yet.
       *[other] This version does not look for commitments outside the tracker yet.
    }
brief-controller = From the controller
brief-controller-round = Its last round: { $when }.
brief-controller-stale = Its last round was { $when }, and it has not gone round since: this may be old.
brief-controller-budget = { $rounds ->
        [one] The budget has been unknown for { $rounds } round: nothing starts by itself until it is read.
       *[other] The budget has been unknown for { $rounds } rounds: nothing starts by itself until it is read.
    }
brief-controller-budget-since = Unknown since { $when }.
brief-controller-open = It could not open this by itself today; open it by hand:
brief-controller-resume = Stopped, and it will not resume this by itself:
brief-why = Why
brief-how = How
brief-where = Where
brief-prompt = Prompt
brief-session-id = Session
brief-resumes = Can resume
dossier-not-repo = the folder is not the root of a git repository
dossier-no-upstream = the branch has no upstream
dossier-detached = HEAD is detached: no branch
dossier-git-failed = git did not answer
dossier-on-branch = on the branch
