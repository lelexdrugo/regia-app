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
