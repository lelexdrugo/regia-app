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

## `init plan` and `init derive` in the terminal: the detail of each item of the plan, in the prototype's words, and
## the lines of the text. A message takes one variable at most: the code joins the parts of a detail. A repo is
## `owner/repo`. A part that starts with a blank starts with `{" "}`, which Fluent keeps.

init-gh-signed-out = gh is not signed in or does not answer: { $said }
init-gh-sign-in = . Run `gh auth login -s { $scopes }`.
init-gh-unreadable = the answer of `gh api user` does not read: { $error }
init-gh-signed-in-as = signed in as { $login }
init-gh-scopes-hidden = ; the token does not show its scopes (a fine-grained one?): not checked
init-gh-scopes = , scopes { $scopes }
init-gh-scope-missing = , missing the scope { $scopes }
init-gh-scope-or = or
init-gh-scopes-seen = { " " }(scopes: { $scopes })
init-gh-refresh = . Run `gh auth refresh -s { $scopes }`.
init-gh-exit = gh exited with { $code }
init-unchecked-signed-out = cannot be checked: gh is not signed in
init-repo-missing = { $repo } does not exist; it would be created private
init-repo-public = { $repo } exists but is public: { -product } does not make it private by itself; the owner decides (Settings → Danger Zone → Change visibility) or picks another name with --repo
init-repo-private = { $repo }, private
init-project-named = Project «{ $title }»
init-of-owner = { " " }of { $owner }
init-unchecked-project-unread = cannot be checked: the Project could not be read
init-project-none = no Project «{ $title }»
init-owner-unknown = owner { $owner } unknown
init-would-create = ; it would be created
init-status-needs-project = depends on the Project, which is missing
init-project-field-born = the field «{ $field }» comes with the first project (a single-select field needs at least one option)
init-projects-titled = more than one Project titled «{ $title }»
init-projects-rename = : rename or delete the extra ones
init-unchecked-ambiguous = cannot be checked: the Project is ambiguous
init-status-field-missing = the Project has no Status field; it would be created
init-status-field-kind = the Status field is a { $kind }, not a single select
init-status-options-missing = missing or named otherwise: { $names }
init-status-options-found = { " " }(found: { $names })
init-status-options-extra = ; extra options, left alone: { $names }
init-field-options = { $count } option(s)
init-field-named = the field «{ $field }»
init-field-kind = { " " }is a { $kind }, not a single select
init-project-field-missing = the Project has no field «{ $field }»
init-would-create-with = ; it would be created with { $options }
init-unchecked-repo-unread = cannot be checked: the repo could not be read
init-label-needs-repo = depends on the repo, which is missing
init-labels-of = labels of { $repo }
init-label-color = { " " }(color { $color })
init-no-skills = no skill in { $folder }
init-no-skills-folder = no skill to compare: no dossier names their folder
init-skill-copies-same = same copies in ~/.claude/skills and ~/.agents/skills
init-copy-missing = missing
init-copy-same = same
init-copy-different = different
init-skill-install = . Run `{ $command }`.
init-project-views = The Project's views can neither be created nor read through the API (ADR 0013 §3): open the Project on GitHub and create by hand the views you use (for example a table grouped by Status and a board).

## The plan in text: `init plan: <owner>/<repo>, Project «<title>»`, then per item
## `  [<outcome>] <item>  (action: <action>)` and its detail on the next line.

init-plan-header = init plan: { $tracker }
init-plan-project = , Project «{ $title }»
init-plan-action = (action: { $action })
init-outcome-present = present
init-outcome-missing = missing
init-outcome-different = different
init-outcome-error = error
init-action-none = none
init-action-create = create
init-action-update = update
init-action-manual = manual
init-action-install = install

## `init derive`: what happened to the local configuration, on stdout, or on stderr with `--json`; the refusals
## always on stderr.

init-derive-not-ready = nothing to derive: the tracker is not all present.
init-derive-same = { $path }: already the same, left alone
init-derive-different = error: { $path } exists and differs; use --overwrite to replace it
init-derive-written = { $path }: written
init-anchor-mismatch = { $file } names another tracker than the one this app is anchored to
init-anchor-file = {" "}({ $anchor })
init-anchor-found = in the file: { $tracker }
init-anchor-anchored = anchored:    { $tracker }
init-anchor-again = if the tracker really moved, run `init derive` again to anchor the app to it
