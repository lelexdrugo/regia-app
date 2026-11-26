### Il testo di `controller run`: una riga per giro, come la sintesi del giro del prototipo, e ciò che il residente
### dice al suo utente. Un messaggio prende al più una variabile: le parti le unisce il codice. Le parole del formato
### del controller (decisioni, esiti, azioni) sono dati, e restano come sono.

# Un giro: "<giro> <istante>: <precheck> -> <decisione>", poi fra parentesi quadre i lanci, le operazioni, la
# sveglia, e il risveglio dopo una sospensione.
controller-round = giro { $round }
controller-precheck = precheck exit { $exit }
controller-no-precheck = precheck non eseguito
controller-operations = [operazioni: { $list }]
# "[sveglia: <esito>]": l'esito è "ok" o "fallita (<errore>)", poi ", token rinnovato".
controller-wake = [sveglia: { $outcome }]
controller-wake-ok = ok
controller-wake-failed = fallita ({ $error })
controller-wake-renewed = , token rinnovato
controller-after-suspension = (risveglio)
# "<allarme> <dal><ferma> <motivo>".
controller-budget-alarm = ATTENZIONE: budget di Claude sconosciuto da { $rounds } giri
controller-budget-since = (dal { $since })
controller-budget-stand-still = : { -product } non lancia nulla finché la sonda non risponde.
controller-budget-reason = Motivo: { $reason }
# "<giro>: errore: <errore>".
controller-round-failed = giro { $round }: errore:
controller-wakeup = risveglio dopo una sospensione: giro subito
controller-log-not-written = controller: log non scritto: { $error }
controller-status-not-written = controller: stato non scritto: { $error }
# Le uscite.
controller-stopped = controller: interrotto
# "<altro> <titolare>".
controller-another = controller: un altro controller è attivo
controller-another-holder = ({ $holder })
controller-lock-lost = controller: { $error }
controller-failed = controller: { $error }
controller-bad-option = controller: uso errato: l'intervallo, i passi di un giro, il tetto del passo e il timeout del precheck devono essere positivi
controller-fixed-clock = { $variable } ferma l'orologio: vale per un solo giro (--once)
# Un giro in ombra.
controller-shadow = giro in ombra: una copia nuova dello stato in { $folder }, e non si lancia nulla
controller-shadow-released = nella copia, il lock del controller di { $holder } è rilasciato
# Le sessioni fermate dal limite d'uso (#470): "[ripresa: <sessione> (<adapter>), ...]" dopo i lanci, e
# "[sospese fino a <istante>]" prima delle operazioni.
controller-resumes = [ripresa: { $list }]
controller-suspended-until = [sospese fino a { $until }]
