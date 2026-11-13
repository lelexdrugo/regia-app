### The moviola: the projects' story on a timeline.

moviola-title = Moviola
moviola-sub = { $n ->
        [one] { $n } momento su { $p } progetti, in sola lettura.
       *[other] { $n } momenti su { $p } progetti.
    }
moviola-sub-empty = Ancora nessun momento: il cronista scrive il primo a fine giornata.
moviola-readonly = Sola lettura. Scritta dal cronista ogni sera: ultimo giro { $when }
moviola-lanes-note = Più larga è la corsia, più worker lavoravano insieme quel giorno. Il puntino accanto a un segno: difetti aperti (rosso) o tutti chiusi (verde).
moviola-search-clear = Svuota la ricerca
moviola-filter-clear = Togli i filtri
moviola-then = Poi
moviola-lanes-label = Corsie dei progetti
moviola-search = Trova un momento
moviola-search-placeholder = Trova un momento: una parola, un #numero
moviola-filter-type = Filtra per tipo
moviola-filter-all = Tutti
moviola-moments = Momenti
moviola-col-when = Quando
moviola-col-type = Tipo
moviola-col-what = Che cosa
moviola-col-project = Progetto
moviola-noresult = Nessun momento corrisponde. Prova una parola sola.
moviola-empty-table = Nessun momento ancora.
moviola-detail = Momento scelto
moviola-nothing-title = Scegli un momento
moviola-nothing-body = Clicca un segno sulle corsie o una riga dell'elenco: qui leggi prima il perché, poi i numeri.
moviola-why = Perché
moviola-numbers = I numeri
moviola-health = Salute
moviola-round = Round
moviola-defects = Difetti
moviola-defects-value = { $found } trovati, { $closed } chiusi a questa data
moviola-from = Viene da
moviola-refs = Riferimenti
moviola-copy = Copia per la chat
moviola-copied = Pacchetto copiato: incollalo in una chat.
moviola-discuss = Discutine in una sessione
moviola-playhead = Testina di lettura
moviola-prev = Momento precedente
moviola-next = Momento successivo
moviola-zoom-in = Avvicina
moviola-zoom-out = Allontana
moviola-state-at = A questa data: momenti { $moments } · gate aperti { $gates } · a rischio { $risk } · worker in parallelo { $workers }
moviola-package = { -product } · { $project } · nodo { $id } ({ $type }, { $when })
    Sto guardando: { $title }. { $why }
    Riferimenti: { $refs }
    Domanda:
    Carica la skill { -product } e leggi i riferimenti prima di rispondere.
type-milestone = Traguardo
type-decision = Decisione presa
type-gate = Decisione chiesta
type-exception = Eccezione
type-exploration = Esplorazione
type-return = Rientro
type-abandoned = Abbandono
health-healthy = in salute
health-at-risk = a rischio
health-waiting = aspetta te

## The moviola of #486: the projects (L0), then a project's lanes by direction, the playhead and the tape.

moviola-projects-sub = { $n ->
        [one] Un progetto con una cartella su questo computer.
       *[other] { $n } progetti con una cartella su questo computer.
    }
moviola-projects-empty = Nessun progetto ha ancora una cartella su questo computer.
moviola-projects-note = Apri un progetto per vederne la storia: una corsia per ogni direzione presa.
moviola-error-lead = Non riesco a leggere la moviola:
moviola-project-none = ancora nessun momento
moviola-project-counts = { $milestones ->
        [one] un momento
       *[other] { $milestones } momenti
    } · { $directions ->
        [one] una direzione
       *[other] { $directions } direzioni
    }
moviola-project-span = { $from } → { $to }
moviola-project-last = Ultimo
moviola-back = Tutti i progetti
moviola-timeline-sub = { $n ->
        [one] Un momento
       *[other] { $n } momenti
    } su { $lanes ->
        [one] una corsia
       *[other] { $lanes } corsie
    }, in sola lettura.
moviola-lanes-directions = Corsie per direzione
moviola-stage = Le corsie del progetto: trascina per spostarle, scorri per avvicinare, le frecce vanno da un momento all'altro.
moviola-stage-hint = Scegli un momento: clicca un segno, o usa le frecce.
moviola-fit = Mostra tutta la storia
moviola-lane-main = principale
moviola-lane-active = attiva
moviola-lane-abandoned = abbandonata
moviola-lane-rejoined = rientrata

## Il fotogramma di un momento (#487): prima la sintesi, poi il perché, che cosa è cambiato, come ci si è arrivati, i riferimenti.

moviola-frame-close = Chiudi il momento
moviola-in-brief = In breve
moviola-changed = Che cosa è cambiato
moviola-open-decisions = Decisioni aperte
moviola-risks = Rischi
moviola-next-step = Prossimo passo
moviola-claims = Claim cambiati
moviola-deviations = Deviazioni dal piano
moviola-deviation = { $plan } → { $deviation }: { $reason } ({ $impact })
moviola-none = nessuna voce
moviola-not-declared = non dichiarato
moviola-opens-lane = Apre la corsia «{ $lane }». { $reason }
moviola-closes-lane = Chiude la corsia «{ $lane }» ({ $status }). { $reason }
moviola-process = Processo
moviola-checks = Controlli
moviola-found = Difetti trovati
moviola-closed = Chiusi
moviola-work = Come ci si è arrivati
moviola-work-sub = Chi ha lavorato, per task, dagli aggregati della moviola.
moviola-no-tasks = Il momento non cita nessun task.
moviola-no-aggregate = Nessun aggregato per questo task: il suo processo non è dichiarato.
moviola-task-head = #{ $task } · round { $round } · sottoagenti { $subagents }
moviola-model-use = { $agent } · { $model } · effort { $effort } · { $sessions ->
        [one] un dispatch
       *[other] { $sessions } dispatch
    }
moviola-model-cited = { $agent } · { $model } · effort { $effort } · { $sessions ->
        [one] una sessione citata dal report, lavoro in sessione
       *[other] { $sessions } sessioni citate dal report, lavoro in sessione
    }
moviola-no-session = Nessuna sessione registrata qui: chi ha lavorato non si sa su questa macchina.
moviola-check = controllo { $kind }: { $outcome }
moviola-check-model = controllo { $kind }: { $outcome } · { $model }
moviola-task-defects = difetti trovati { $found } · chiusi { $closed }
moviola-task-closes = chiude { $refs }
moviola-tokens = token: in { $input } · out { $output } · cache letta { $read } · cache scritta { $written }
moviola-no-tokens = Token non registrati qui: vengono dalle sessioni che questa macchina legge.
moviola-issues = Issue
moviola-commits = Commit
moviola-reports = Report
moviola-not-here = non presente su questa macchina
moviola-tracker-project = Il Project del tracker
moviola-notes-loading = Leggo le note del momento…
moviola-notes-failed = Le note del momento non si leggono:
moviola-notes-none = Il cronista non ha scritto note per questo momento.
moviola-link-note = Questa versione non apre ancora il browser. L'indirizzo:
