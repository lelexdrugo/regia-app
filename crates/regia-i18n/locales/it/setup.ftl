### The guided setup and the tracker's plan.

setup-steps = Passi del setup
setup-step-welcome = Lingua
setup-step-tools = Strumenti
setup-step-github = GitHub
setup-step-tracker = Tracker
setup-step-services = Servizi di agenti
setup-step-finish = Fine
setup-back = Indietro
setup-next = Continua
setup-begin = Comincia
setup-welcome-title = Prepariamo { -product }
setup-welcome-lead = Qui decidi che cosa fanno gli agenti sui tuoi progetti e rivedi come ci sono arrivati. Prima servono pochi controlli su questa macchina.
setup-language = Lingua dell'app
setup-promise-time = Circa 3 minuti.
setup-promise-nowrite = Non scrive niente su GitHub prima che tu abbia visto l'elenco.
setup-promise-secrets = I token restano in gh e nel portachiavi del sistema, mai in un file.
setup-tools-title = Che cosa c'è su questa macchina
setup-tools-lead = Ho cercato gli strumenti che servono. Orca è facoltativa.
setup-tools-lead-missing = Manca gh, il programma che parla con GitHub. Installalo, poi ricontrolla: gli altri restano come sono.
setup-tools-gh-missing = Non trovato nel PATH
setup-tools-gh-install = Scarica gh
setup-tools-orca = Trovata: le sessioni si apriranno lì
setup-tools-orca-none = Non trovata: gli agenti partiranno senza interfaccia
setup-tools-recheck = Ricontrolla
setup-tools-blocked = Serve gh per continuare.
setup-github-title = Accedi a GitHub
setup-github-lead = Il tracker vive su GitHub. L'accesso passa da gh e dal tuo browser: l'app non vede mai la password.
setup-github-done = Accesso fatto. Il token resta a gh.
setup-github-code = Copia questo codice e incollalo nella pagina che si apre.
setup-github-open = Apri GitHub nel browser
setup-github-note = Questa schermata prosegue da sola quando GitHub conferma.
setup-github-as = accesso come { $who }
setup-github-scope-ok = repo, project
setup-github-scope-missing = manca project: rilancia gh auth refresh -s project
setup-tracker-title = Il tracker su GitHub
setup-tracker-lead = { $n ->
        [one] Manca { $n } cosa: eccola, prima di toccarla.
       *[other] Mancano { $n } cose: { $create } le creo io, { $install } la installo, { $manual } tocca a te. Non succede niente finché non confermi, e il repo nasce privato.
    }
setup-tracker-lead-existing = Il tracker esiste già. Lo uso così com'è: nessuna scrittura.
setup-tracker-lead-error = Non posso leggere il tracker: il token non ha il permesso project. Nulla è stato scritto.
setup-tracker-dry = Mostra i comandi che lancerei
setup-tracker-apply = { $n ->
        [one] Crea { $n } cosa e installa la skill
       *[other] Crea { $n } cose e installa la skill
    }
setup-tracker-applied = { $n ->
        [one] Creata { $n } cosa. Il piano rilanciato è pulito.
       *[other] Create { $n } cose. Il piano rilanciato è pulito.
    }
setup-tracker-fix = Rilancia gh auth refresh -s project, poi torna qui.
plan-scope-environment = Ambiente
plan-scope-tracker = Tracker
plan-scope-local = Su questa macchina
plan-scope-manual = A mano
plan-outcome-present = presente
plan-outcome-missing = manca
plan-outcome-different = diverso
plan-outcome-error = errore
plan-action-create = da creare
plan-action-update = da aggiornare
plan-action-manual = a mano
plan-action-install = da installare
plan-gh-ok = autenticato, con i permessi repo e project
plan-repo = il repo delle issue, privato
plan-project = la bacheca degli stati
plan-status = il campo Status con 7 stati
plan-label-goal = l'etichetta dell'Obiettivo di un progetto
plan-label-draft = segnaposto: spec da scrivere
plan-label-ready = spec completa, eseguibile se approvata
plan-label-parked = fuori dall'arretrato da scodare
plan-label-session = il memo propone una sessione con te
plan-skill = la skill che gli agenti leggono
plan-views = le viste del Project non si creano via API: vanno fatte una volta a mano
plan-scope = non leggibile: manca il permesso project
setup-services-title = Con quali agenti lavori
setup-services-lead = Il principale è quello che l'orchestratore fa partire. Lo cambi quando vuoi dalle impostazioni.
setup-finish-title = È tutto pronto, tranne una cosa
setup-finish-lead = La sala di regia è aperta. Le viste del Project sono l'unico passo a mano, e puoi farlo anche dopo.
setup-finish-tracker = repo privato, bacheca e stati creati
setup-finish-skill = installata per Claude e Codex
setup-finish-views = Viste del Project
setup-finish-views-open = Apri il Project
setup-finish-go = Apri la sala di regia
setup-tools-found = { $version }: trovato
setup-tools-signed-in = { $version }: trovato, accesso fatto

## `init plan` and `init derive` in the terminal: the detail of each item of the plan, in the prototype's words, and
## the lines of the text. A message takes one variable at most: the code joins the parts of a detail. A repo is
## `owner/repo`. A part that starts with a blank starts with `{" "}`, which Fluent keeps.

init-gh-signed-out = gh non è autenticato o non risponde: { $said }
init-gh-sign-in = . Esegui `gh auth login -s { $scopes }`.
init-gh-unreadable = risposta di `gh api user` illeggibile: { $error }
init-gh-signed-in-as = autenticato come { $login }
init-gh-scopes-hidden = ; scope non esposti dal token (a grana fine?): non verificati
init-gh-scopes = , scope { $scopes }
init-gh-scope-missing = , manca lo scope { $scopes }
init-gh-scope-or = o
init-gh-scopes-seen = { " " }(scope: { $scopes })
init-gh-refresh = . Esegui `gh auth refresh -s { $scopes }`.
init-gh-exit = gh esce con { $code }
init-unchecked-signed-out = non verificabile: gh non è autenticato
init-repo-missing = { $repo } non esiste; si creerebbe privato
init-repo-public = { $repo } esiste ma è pubblico: la { -product } non lo rende privato da sola, decide il titolare (Settings → Danger Zone → Change visibility) o sceglie un altro nome con --repo
init-repo-private = { $repo }, privato
init-project-named = Project «{ $title }»
init-of-owner = { " " }di { $owner }
init-unchecked-project-unread = non verificabile: il Project non si è potuto leggere
init-project-none = nessun Project «{ $title }»
init-owner-unknown = owner { $owner } sconosciuto
init-would-create = ; si creerebbe
init-status-needs-project = dipende dal Project, che manca
init-project-field-born = il campo «{ $field }» nasce con il primo progetto (un campo a scelta singola vuole almeno un'opzione)
init-projects-titled = più Project intitolati «{ $title }»
init-projects-rename = : rinomina o elimina quelli in più
init-unchecked-ambiguous = non verificabile: Project ambiguo
init-status-field-missing = il Project non ha il campo Status; si creerebbe
init-status-field-kind = il campo Status è un { $kind }, non a scelta singola
init-status-options-missing = mancano o hanno un altro nome: { $names }
init-status-options-found = { " " }(trovate: { $names })
init-status-options-extra = ; opzioni in più, non toccate: { $names }
init-field-options = { $count } opzioni
init-field-named = il campo «{ $field }»
init-field-kind = { " " }è un { $kind }, non a scelta singola
init-project-field-missing = il Project non ha il campo «{ $field }»
init-would-create-with = ; si creerebbe con { $options }
init-unchecked-repo-unread = non verificabile: il repo non si è potuto leggere
init-label-needs-repo = dipende dal repo, che manca
init-labels-of = etichette di { $repo }
init-label-color = { " " }(colore { $color })
init-no-skills = nessuna skill in { $folder }
init-no-skills-folder = nessuna skill da confrontare: nessun dossier ne indica la cartella
init-skill-copies-same = copie uguali in ~/.claude/skills e ~/.agents/skills
init-copy-missing = mancante
init-copy-same = uguale
init-copy-different = diversa
init-skill-install = . Esegui `{ $command }`.
init-project-views = Le viste del Project non si creano né si leggono via API (ADR 0013 §3): apri il Project su GitHub e crea a mano le viste che usi (per esempio una tabella raggruppata per Status e una bacheca).

## The plan in text: `piano di init: <owner>/<repo>, Project «<title>»`, then per item
## `  [<outcome>] <item>  (azione: <action>)` and its detail on the next line.

init-plan-header = piano di init: { $tracker }
init-plan-project = , Project «{ $title }»
init-plan-action = (azione: { $action })
init-outcome-present = presente
init-outcome-missing = mancante
init-outcome-different = diverso
init-outcome-error = errore
init-action-none = nessuna
init-action-create = crea
init-action-update = aggiorna
init-action-manual = manuale
init-action-install = installa

## `init derive`: what happened to the local configuration, on stdout, or on stderr with `--json`; the refusals
## always on stderr.

init-derive-not-ready = niente da ricavare: il tracker non è tutto presente.
init-derive-same = { $path }: già uguale, non toccato
init-derive-different = errore: { $path } esiste ed è diverso; usa --overwrite per sostituirlo
init-derive-written = { $path }: scritto
