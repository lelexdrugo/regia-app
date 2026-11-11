### Text written by others, as a view shows it (threat model §3.7): who wrote it, and what stands in for
### what is not shown. `$host` is an address in ASCII.

content-origin = { $origin ->
        [agent] Scritto da un agente
        [owner] Scritto da te
       *[unknown] Autore sconosciuto
    }
content-image = Immagine omessa ({ $host })
content-image-nohost = Immagine omessa
content-link-refused = { $reason ->
        [scheme] collegamento non aperto: non è https
        [credentials] collegamento non aperto: contiene una password
        [too_long] collegamento non aperto: troppo lungo
       *[invalid] collegamento non aperto
    }
content-truncated = Il testo continua oltre { $limit }: qui ne vedi l'inizio.
