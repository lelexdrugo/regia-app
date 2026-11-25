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

## La ricerca di un momento (#487): una parola, un #numero; Esc la svuota, Invio apre il primo trovato.

moviola-search-count = { $n ->
        [one] un momento su { $total }
       *[other] { $n } momenti su { $total }
    }
moviola-results = Momenti trovati
moviola-search-none = Nessun momento corrisponde a «{ $query }». La ricerca vuole ogni parola nel titolo, nel tipo o nella corsia di un momento, e ogni #numero fra i task che cita.
moviola-search-none-task = Nessun momento cita il task #{ $task }.

## «Copia per la chat» nel fotogramma (#487): cinque righe, solo puntatori, sotto i 1.500 caratteri.

moviola-pack-head = { -product } · { $project } · nodo { $id } ({ $kind }, { $day })
moviola-pack-looking = Sto guardando: { $what }
moviola-pack-references = Riferimenti: { $list }
moviola-pack-none = nessuno
moviola-pack-more = … e altri { $n }
moviola-pack-question = Domanda: <da completare>
moviola-pack-skill = Carica la skill { -product } e leggi i riferimenti prima di rispondere.
moviola-pack-label = Il pacchetto per la chat
moviola-pack-note = Il pacchetto è selezionato: copialo e incollalo in una chat. L'app non scrive ancora negli appunti.

## La parte della moviola nell'aiuto su ? (#487): i suoi tasti e i segni del palco, come li aveva la guida del prototipo.

moviola-help-title = La moviola
moviola-keys-arrows = Sul palco: il momento prima o dopo
moviola-keys-lanes = Sul palco: il momento più vicino nella corsia sopra o sotto
moviola-keys-ends = Sul palco: il primo o l'ultimo momento
moviola-keys-zoom = Sul palco: avvicina, allontana, tutta la storia
moviola-keys-enter = Nella ricerca: apre il primo momento trovato
moviola-keys-escape = Svuota la ricerca; sul palco o nel fotogramma, lascia il momento
moviola-word-kinds-term = I sette tipi
moviola-word-kinds = Un cerchio pieno è un traguardo, un rombo una decisione presa, un quadrato ambra una decisione che aspetta te, un triangolo rosso un'eccezione, un cerchio tratteggiato un'esplorazione, una punta di freccia un rientro nella linea principale, una croce una strada abbandonata.
moviola-word-health-term = Salute
moviola-word-health = Un alone attorno a un momento a rischio, o che aspetta te. Un momento in salute non ne ha: il colore segnala, non decora.
moviola-word-rings-term = Anelli
moviola-word-rings = Un anello per ogni round di ciascun task che il momento cita, sopra e sotto il segno; tratteggiato quando il task non dichiara il round. Il lavoro in parallelo allarga la corsia.
moviola-word-badges-term = Badge
moviola-word-badges = ✓ un controllo riuscito, ? un controllo non concluso, D3 tre difetti trovati, 1/3 uno chiuso su tre.
moviola-word-undeclared-term = Non dichiarato
moviola-word-undeclared = Un valore che nessuno ha scritto. Il palco non disegna nulla, il fotogramma dice «non dichiarato»: non vuol dire mai zero.
moviola-word-lanes-term = Corsie
moviola-word-lanes = Una per ogni direzione che il progetto ha preso. Una linea piena è ancora aperta, una tratteggiata è stata abbandonata, una punteggiata è rientrata nella linea principale.
moviola-word-playhead-term = Testina
moviola-word-playhead = La linea ambra con la sua ora: la data che stai leggendo. Trascinala sul palco o sul nastro.
moviola-word-joint-term = Giunta
moviola-word-joint = ✂ sul nastro: giorni senza momenti, tagliati come in un montaggio.

## La panoramica di un progetto (#598): i filtri, la finestra temporale, lo stato alla testina, le direzioni.

moviola-filters = Filtri
moviola-filter-mode = Quanto filtrare
moviola-filter-basic = Base
moviola-filter-advanced = Avanzati
moviola-filter-who = Chi ha lavorato
moviola-filter-who-any = Chiunque
moviola-filter-who-use = { $agent } · { $model }
moviola-filter-who-cited = { $agent } · { $model } (in sessione)
moviola-filter-lane = Corsia
moviola-filter-lane-any = Tutte le corsie
moviola-filter-lane-option = { $lane } · { $status }
moviola-filter-health-any = Ogni salute
moviola-filter-declared = Con controlli o difetti dichiarati
moviola-filter-hidden = I filtri nascondono tutti i momenti che la ricerca trova.
moviola-window = Finestra temporale
moviola-window-range = { $from } → { $to }
moviola-window-whole = Tutta la storia, { $from } → { $to }
moviola-window-hint = Trascina sulla striscia per una finestra di tempo, i suoi capi per allargarla, il centro per spostarla; un doppio clic mostra tutta la storia.
moviola-at = Alla testina
moviola-at-counts = { $before } prima · { $after } dopo
moviola-at-none = Ancora nessun momento
moviola-at-last = Ultimo momento
moviola-at-open = Direzioni aperte
moviola-at-open-hint = Derivato: una direzione si chiude al primo abbandono o rientro con un genitore nella sua corsia.
moviola-at-open-none = nessuna
moviola-at-decisions = Decisioni aperte nelle sue note
moviola-at-decisions-count = { $n ->
        [one] { $first }
       *[other] { $n } · la prima: { $first }
    }
moviola-at-decisions-none = nessuna nelle sue note
moviola-at-decisions-reading = leggo le sue note…
moviola-at-decisions-failed = le sue note non si sono potute leggere
moviola-directions = Direzioni
moviola-direction-main-title = Linea principale
moviola-direction-main = La linea del progetto: le altre corsie se ne staccano e vi rientrano.
moviola-direction-count = { $n ->
        [one] un momento
       *[other] { $n } momenti
    }
moviola-direction-opened = Aperta da
moviola-direction-abandoned = Abbandonata con
moviola-direction-rejoined = Rientrata con
moviola-direction-no-reason = Nessun motivo registrato.
moviola-direction-no-record = Nessun record nomina questa corsia: solo i suoi momenti.
moviola-direction-empty = Ancora nessun momento su questa corsia.
