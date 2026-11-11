### The guided setup and the tracker's plan.

setup-steps = Setup steps
setup-step-welcome = Language
setup-step-tools = Tools
setup-step-github = GitHub
setup-step-tracker = Tracker
setup-step-services = Agent services
setup-step-finish = Finish
setup-back = Back
setup-next = Continue
setup-begin = Start
setup-welcome-title = Let's set up { -product }
setup-welcome-lead = This is where you decide what agents do on your projects and review how they got there. First, a few checks on this machine.
setup-language = App language
setup-promise-time = About 3 minutes.
setup-promise-nowrite = It writes nothing on GitHub before you've seen the list.
setup-promise-secrets = Tokens stay in gh and in the system keychain, never in a file.
setup-tools-title = What's on this machine
setup-tools-lead = I looked for the tools you need. Orca is optional.
setup-tools-lead-missing = gh, the program that talks to GitHub, is missing. Install it, then check again: the others stay as they are.
setup-tools-gh-missing = Not found on PATH
setup-tools-gh-install = Download gh
setup-tools-orca = Found: sessions will open there
setup-tools-orca-none = Not found: agents will start with no interface
setup-tools-recheck = Check again
setup-tools-blocked = gh is needed to continue.
setup-github-title = Sign in to GitHub
setup-github-lead = The tracker lives on GitHub. Sign-in goes through gh and your browser: the app never sees the password.
setup-github-done = Signed in. The token stays with gh.
setup-github-code = Copy this code and paste it on the page that opens.
setup-github-open = Open GitHub in the browser
setup-github-note = This screen moves on by itself when GitHub confirms.
setup-github-as = signed in as { $who }
setup-github-scope-ok = repo, project
setup-github-scope-missing = project is missing: run gh auth refresh -s project
setup-tracker-title = The tracker on GitHub
setup-tracker-lead = { $n ->
        [one] { $n } thing is missing: here it is, before touching it.
       *[other] { $n } things are missing: I create { $create }, I install { $install }, { $manual } is up to you. Nothing happens until you confirm, and the repo starts private.
    }
setup-tracker-lead-existing = The tracker already exists. I'll use it as it is: no writes.
setup-tracker-lead-error = I can't read the tracker: the token lacks the project scope. Nothing has been written.
setup-tracker-dry = Show the commands I would run
setup-tracker-apply = { $n ->
        [one] Create { $n } thing and install the skill
       *[other] Create { $n } things and install the skill
    }
setup-tracker-applied = { $n ->
        [one] { $n } thing created. The plan, run again, is clean.
       *[other] { $n } things created. The plan, run again, is clean.
    }
setup-tracker-fix = Run gh auth refresh -s project, then come back.
plan-scope-environment = Environment
plan-scope-tracker = Tracker
plan-scope-local = On this machine
plan-scope-manual = By hand
plan-outcome-present = present
plan-outcome-missing = missing
plan-outcome-different = different
plan-outcome-error = error
plan-action-create = to create
plan-action-update = to update
plan-action-manual = by hand
plan-action-install = to install
plan-gh-ok = signed in, with the repo and project scopes
plan-repo = the issues repo, private
plan-project = the board of states
plan-status = the Status field with 7 states
plan-label-goal = the label for a project's Goal
plan-label-draft = placeholder: spec to write
plan-label-ready = spec complete, runnable once approved
plan-label-parked = out of the backlog to dequeue
plan-label-session = the memo proposes a session with you
plan-skill = the skill agents read
plan-views = Project views can't be created through the API: make them once by hand
plan-scope = unreadable: the project scope is missing
setup-services-title = Which agents you work with
setup-services-lead = The principal is the one the orchestrator starts. You can change it any time in settings.
setup-finish-title = All set, except one thing
setup-finish-lead = The control room is open. The Project views are the only manual step, and you can do it later.
setup-finish-tracker = private repo, board and states created
setup-finish-skill = installed for Claude and Codex
setup-finish-views = Project views
setup-finish-views-open = Open the Project
setup-finish-go = Open the control room
setup-tools-found = { $version }: found
setup-tools-signed-in = { $version }: found, signed in
