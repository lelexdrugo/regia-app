### The brief: what waits for the owner, what is ready, what runs.

## Shared with the CLI, which prints the same brief.

brief-waiting = Aspetta te (proposta, decisione)
brief-session = Serve una sessione con te
brief-ready = Pronti da eseguire
brief-in-progress = In corso
brief-by-project = Per progetto
brief-nothing = (niente)
brief-loading = Sto leggendo il brief…
brief-error = Non riesco a leggere il brief: { $reason }
brief-partial = Non ancora portati: gli avvisi del controller, dei dossier e di ciò che è fuori dal tracker non si vedono.
brief-cycle = Dipendenze in ciclo, senza ordine del grafo: { $reason }
brief-truncated = Dipendenze lette solo in parte: { $reason }

## The app's own.

brief-title = Brief
brief-finding = { $n ->
        [0] Niente ti aspetta. Gli agenti proseguono da soli.
        [one] { $n } voce ti aspetta. Comincia da #{ $top }: sblocca { $unlocks } task.
       *[other] { $n } voci ti aspettano. Comincia da #{ $top }: sblocca { $unlocks } task.
    }
brief-updated = Tracker letto alle { $when }
brief-to-daily = Fai i giornalieri
brief-ready-empty = Nessun task pronto.
brief-running-empty = Nessun agente al lavoro.
brief-dossiers = Dossier da sistemare
brief-offtracker = Fuori dal tracker
brief-offtracker-more = … e altre { $n }
brief-offtracker-note = Segnalazioni con falsi positivi noti, non decisioni.
brief-range = { $from }–{ $to } di { $total }
brief-more = Mostra altre
brief-empty = Nessuna decisione in attesa.
brief-empty-done = Hai risposto a tutte.
brief-empty-sessions = Restano solo le voci che chiedono una sessione con te.
brief-done = Fatte oggi
brief-done-choice = { $outcome ->
        [approve] approvata
        [discard] scartata
        [defer] rimandata
       *[option] scelta { $choice }
    }
brief-done-writing = sto scrivendo
brief-done-written = scritto
brief-stopped-link = Vedi la console
queue-chain-title = Sblocca subito { $unlocks } task; la catena di lavoro che aspetta è lunga { $chain }
brief-empty-title = La coda è vuota
brief-empty-body = Agenti al lavoro: { $running }. Task pronti: { $ready }. Quando serve una tua scelta, compare qui con un consiglio.
brief-empty-console = Guarda la console
brief-empty-moviola = Rivedi la moviola
queue-unlocks = sblocca { $n }
queue-chain = catena { $n }
dossier-unpushed = { $n ->
        [one] { $n } commit non pushato
       *[other] { $n } commit non pushati
    }
dossier-noremote = senza remoto
dossier-missing = cartella mancante

## The app's queue, its blocks and their states. What others wrote (titles, projects, reasons, folders) is
## shown next to these words, never inside them.

brief-error-lead = Non riesco a leggere il brief:
brief-not-read = Non letto: il brief non si è potuto leggere.
queue-due = entro il { $day }
queue-overdue = scaduto il { $day }
queue-unlocks-total = sblocca { $n } ({ $total } con Inbox e Proposte)
queue-cycle = in ciclo
brief-due-ignored = #{ $n }: la scadenza non è una data AAAA-MM-GG, quindi è ignorata:
brief-not-ported = { $section ->
        [controller] Questa versione non legge ancora gli avvisi del controller.
        [dossier] Questa versione non controlla ancora i dossier.
       *[other] Questa versione non cerca ancora gli impegni fuori dal tracker.
    }
brief-controller = Dal controller
brief-controller-round = Il suo ultimo giro: { $when }.
brief-controller-stale = Il suo ultimo giro è del { $when }, e da allora è fermo: questo può essere vecchio.
brief-controller-budget = { $rounds ->
        [one] Il budget è sconosciuto da { $rounds } giro: non parte nulla da solo finché non si legge.
       *[other] Il budget è sconosciuto da { $rounds } giri: non parte nulla da solo finché non si legge.
    }
brief-controller-budget-since = Sconosciuto dal { $when }.
brief-controller-open = Da aprire a mano, oggi:
brief-controller-resume = Fermata, da riprendere a mano:
brief-why = Perché
brief-how = Come
brief-where = Dove
brief-prompt = Prompt
brief-session-id = Sessione
brief-resumes = Si può riprendere
dossier-not-repo = la cartella non è la radice di un repo git
dossier-no-upstream = il branch non ha un upstream
dossier-detached = HEAD staccato: nessun branch
dossier-git-failed = git non ha risposto
dossier-on-branch = sul branch
