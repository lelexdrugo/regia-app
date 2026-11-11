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
