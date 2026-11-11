### The dailies: the story since last time, then one question at a time.

daily-title = Dailies
daily-since = { $n ->
        [one] Since last time, at { $when }: news on { $n } project.
       *[other] Since last time, at { $when }: news on { $n } projects.
    }
daily-start = { $n ->
        [one] Answer { $n } question
       *[other] Answer { $n } questions
    }
daily-only-story = Story only
daily-story = What happened
daily-story-empty = No news since last time.
daily-still = Idle:
daily-queue = Queue
daily-queue-empty = No questions for you today.
daily-closed = { $n ->
        [one] { $n } closed
       *[other] { $n } closed
    }
daily-reports = { $n ->
        [one] { $n } report
       *[other] { $n } reports
    }
daily-commits = { $n ->
        [one] { $n } commit
       *[other] { $n } commits
    }
report-gate = decision
report-failed = stopped
report-done = done
report-proposal = proposal
daily-per-project = By project
daily-warnings = Warnings
daily-warn-dossiers = { $n ->
        [one] { $n } dossier to fix.
       *[other] { $n } dossiers to fix.
    }
daily-warn-off = { $n ->
        [one] { $n } item off the tracker.
       *[other] { $n } items off the tracker.
    }
daily-questions = One question at a time
daily-progress = { $done } of { $total } answered
daily-position = Question { $i } of { $total }
daily-back-story = Back to the story
daily-finish = Finish here
daily-end-title = Dailies closed
daily-end-sub = Decided: { $n }. Still in the queue: { $left }.
daily-to-brief = Go to the brief
daily-end-applied = Applied
daily-end-none = No answer applied.
daily-end-left = In the queue for next time
daily-end-nothing = Nothing: you've emptied the queue.
daily-end-file = The record
daily-end-file-note = Written and pushed, like today's dailies file from the chat. The next story starts from here.
