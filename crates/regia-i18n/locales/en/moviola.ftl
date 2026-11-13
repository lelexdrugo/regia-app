### The moviola: the projects' story on a timeline.

moviola-title = Moviola
moviola-sub = { $n ->
        [one] { $n } moment across { $p } projects, read only.
       *[other] { $n } moments across { $p } projects.
    }
moviola-sub-empty = No moments yet: the chronicler writes the first at the end of the day.
moviola-readonly = Read only. Written by the chronicler every evening: last run { $when }
moviola-lanes-note = The wider the lane, the more workers ran together that day. The dot beside a mark: open defects (red) or all closed (green).
moviola-search-clear = Clear the search
moviola-filter-clear = Clear the filters
moviola-then = Then
moviola-lanes-label = Project lanes
moviola-search = Find a moment
moviola-search-placeholder = Find a moment: a word, a #number
moviola-filter-type = Filter by type
moviola-filter-all = All
moviola-moments = Moments
moviola-col-when = When
moviola-col-type = Type
moviola-col-what = What
moviola-col-project = Project
moviola-noresult = No moment matches. Try a single word.
moviola-empty-table = No moments yet.
moviola-detail = Selected moment
moviola-nothing-title = Pick a moment
moviola-nothing-body = Click a mark on the lanes or a row of the list: here you read the why first, then the numbers.
moviola-why = Why
moviola-numbers = The numbers
moviola-health = Health
moviola-round = Round
moviola-defects = Defects
moviola-defects-value = { $found } found, { $closed } closed at this date
moviola-from = Comes from
moviola-refs = References
moviola-copy = Copy for chat
moviola-copied = Package copied: paste it into a chat.
moviola-discuss = Discuss it in a session
moviola-playhead = Playhead
moviola-prev = Previous moment
moviola-next = Next moment
moviola-zoom-in = Zoom in
moviola-zoom-out = Zoom out
moviola-state-at = At this date: moments { $moments } · open gates { $gates } · at risk { $risk } · parallel workers { $workers }
moviola-package = { -product } · { $project } · node { $id } ({ $type }, { $when })
    I'm looking at: { $title }. { $why }
    References: { $refs }
    Question:
    Load the { -product } skill and read the references before answering.
type-milestone = Milestone
type-decision = Decision made
type-gate = Decision asked
type-exception = Exception
type-exploration = Exploration
type-return = Return
type-abandoned = Abandoned
health-healthy = healthy
health-at-risk = at risk
health-waiting = waiting for you

## The moviola of #486: the projects (L0), then a project's lanes by direction, the playhead and the tape.

moviola-projects-sub = { $n ->
        [one] One project with a folder on this machine.
       *[other] { $n } projects with a folder on this machine.
    }
moviola-projects-empty = No project has a folder on this machine yet.
moviola-projects-note = Open a project to see its story: one lane for each direction it took.
moviola-error-lead = The moviola could not be read:
moviola-project-none = no moments yet
moviola-project-counts = { $milestones ->
        [one] one moment
       *[other] { $milestones } moments
    } · { $directions ->
        [one] one direction
       *[other] { $directions } directions
    }
moviola-project-span = { $from } → { $to }
moviola-project-last = Last
moviola-back = All projects
moviola-timeline-sub = { $n ->
        [one] One moment
       *[other] { $n } moments
    } on { $lanes ->
        [one] one lane
       *[other] { $lanes } lanes
    }, read only.
moviola-lanes-directions = Lanes by direction
moviola-stage = The project's lanes: drag to move them, scroll to zoom, the arrow keys go from moment to moment.
moviola-stage-hint = Pick a moment: click a mark, or use the arrow keys.
moviola-fit = Show the whole story
moviola-lane-main = main
moviola-lane-active = active
moviola-lane-abandoned = abandoned
moviola-lane-rejoined = rejoined

## The frame of a moment (#487): its summary first, the why, what changed, how it got there, the references.

moviola-frame-close = Close the moment
moviola-in-brief = In brief
moviola-changed = What changed
moviola-open-decisions = Open decisions
moviola-risks = Risks
moviola-next-step = Next step
moviola-claims = Claims that changed
moviola-deviations = Deviations from the plan
moviola-deviation = { $plan } → { $deviation }: { $reason } ({ $impact })
moviola-none = nothing written
moviola-not-declared = not declared
moviola-opens-lane = Opens the lane «{ $lane }». { $reason }
moviola-closes-lane = Closes the lane «{ $lane }» ({ $status }). { $reason }
moviola-process = Process
moviola-checks = Checks
moviola-found = Defects found
moviola-closed = Closed
moviola-work = How it got here
moviola-work-sub = Who worked, by task, from the moviola's aggregates.
moviola-no-tasks = The moment names no task.
moviola-no-aggregate = No aggregate for this task: its process is not declared.
moviola-task-head = #{ $task } · round { $round } · subagents { $subagents }
moviola-model-use = { $agent } · { $model } · effort { $effort } · { $sessions ->
        [one] one dispatch
       *[other] { $sessions } dispatches
    }
moviola-model-cited = { $agent } · { $model } · effort { $effort } · { $sessions ->
        [one] one session cited by the report, work done in session
       *[other] { $sessions } sessions cited by the report, work done in session
    }
moviola-no-session = No session recorded here: who worked is not known on this machine.
moviola-check = check { $kind }: { $outcome }
moviola-check-model = check { $kind }: { $outcome } · { $model }
moviola-task-defects = defects found { $found } · closed { $closed }
moviola-task-closes = closes { $refs }
moviola-tokens = tokens: in { $input } · out { $output } · cache read { $read } · cache written { $written }
moviola-no-tokens = Tokens not recorded here: they come from the sessions this machine reads.
moviola-issues = Issues
moviola-commits = Commits
moviola-reports = Reports
moviola-not-here = not on this machine
moviola-tracker-project = The tracker's Project
moviola-notes-loading = Reading the moment's notes…
moviola-notes-failed = The moment's notes could not be read:
moviola-notes-none = The chronicler wrote no notes for this moment.
moviola-link-note = This version does not open the browser yet. The address:

## The search of a moment (#487): a word, a #number; Esc empties it, Enter opens the first found.

moviola-search-count = { $n ->
        [one] one moment of { $total }
       *[other] { $n } moments of { $total }
    }
moviola-results = Moments found
moviola-search-none = No moment matches «{ $query }». The search wants each word in a moment's title, kind or lane, and each #number among the tasks it names.
moviola-search-none-task = No moment names task #{ $task }.
