### Words every screen uses: states, actions, errors, toasts and dates.

state-inbox = Inbox
state-proposal = Proposta
state-approved = Approvato
state-in_progress = In corso
state-decision = Decisione
state-done = Fatto
state-discarded = Scartato
state-session = Sessione
action-copy = Copia
loading = Caricamento
unit-minutes = minuti
error-block = Non ho potuto leggere { $what }. Ultimo dato buono: { $when }.
error-retry = Riprova
error-retry-done = Nuovo tentativo avviato
error-what-brief = il tracker
error-what-daily = i fatti dei progetti
error-what-moviola = le milestone
error-what-console = il registro dei passi
error-brief-sub = GitHub non risponde: le decisioni restano dove sono, nessuna è andata persa.
error-brief-detail = Quando il tracker torna a rispondere, qui si apre il primo memo.
toast-decided = #{ $n }: { $outcome ->
        [approve] approvata
        [discard] scartata
       *[option] scelta { $choice }
    }.
toast-seconds = Lo scrivo fra { $s } s
toast-undone = #{ $n } è di nuovo in coda. Non ho scritto niente.
toast-undone-previous = #{ $n }: resta deciso { $choice }.
toast-undo-none = Niente da annullare. Le decisioni già scritte si cambiano da «Fatte oggi».
toast-deferred = #{ $n } rimandata al prossimo giornaliero.
toast-undo = Annulla
toast-copied = Copiato
toast-orca = Orca ha aperto la sessione.
toast-stopped = Agente fermato.
toast-controller-on = Controller acceso.
toast-controller-off = Controller fermo. Gli agenti già partiti finiscono il loro lavoro.

## Dates: one format for each use, the same on every screen (#439/D30). `$when` is a date. Only the
## webview formats these: Fluent in Rust has no DATETIME.

date-day = { DATETIME($when, day: "numeric", month: "short") }
date-time = { DATETIME($when, hour: "2-digit", minute: "2-digit") }
date-day-time = { DATETIME($when, day: "numeric", month: "short", hour: "2-digit", minute: "2-digit") }
