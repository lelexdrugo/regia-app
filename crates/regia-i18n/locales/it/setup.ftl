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
