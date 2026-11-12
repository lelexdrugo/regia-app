### The budget policy: `policy show`, `set`, `reset` and `proposals`. A message takes one variable at
### most: the code joins the parts of a line, and puts "- " before each item of the policy.

## Sources of the values, in the title.

policy-source-file = file
policy-source-file-local = file+locale
policy-source-default = default
policy-source-default-local = default+locale

## The policy in use.

policy-title = Politica di budget in uso (fonte: { $source }):
policy-automatic = lavoro automatico: sotto il { $pct }% della finestra di 5 h di Claude
policy-backfill = riempimento (passi accesi solo da idee nuove in Inbox): sotto il { $pct }%
policy-backfill-off = riempimento: nessuna soglia separata, vale quella del lavoro automatico
policy-drain-on = arretrato da scodare (Inbox e abbozzi): acceso
policy-drain-off = arretrato da scodare (Inbox e abbozzi): spento ({ $file })
# `$caps` is the two parts below, joined by ", ".
policy-caps = tetti di Proposte: { $caps }
policy-caps-open = { $n } aperte per progetto
policy-caps-new = { $n } nuove in 7 giorni
policy-caps-off = tetti di Proposte: spenti
policy-local-none = override locale: nessuno
# `$when` is local time, to the minute. A note follows, after " · ".
policy-local-until = override locale: attivo fino a { $when }
policy-local-unbounded = override locale: attivo finché non si azzera
# `$detail` is the reason, or the expiry.
policy-local-expired = override locale: scaduto ({ $detail })
policy-local-unreadable = override locale: illeggibile ({ $detail })
policy-local-ignored = override locale: ignorato ({ $detail })
policy-defaults = Attenzione: { $file } non si legge, valgono i default del codice.

## `policy set` and `policy reset`.

policy-not-written = override non scritto: { $reason }
policy-removed = Override locale tolto.
policy-nothing-to-remove = Nessun override locale da togliere.

## `policy proposals`. The line is "<project>: <open>; <new>." and a count is "<n> <out of>".

policy-proposals-open = Proposte aperte { $count }
policy-proposals-new = nuove negli ultimi 7 giorni { $count }
policy-out-of = su { $cap }
policy-new-unknown = non disponibile
policy-room-caps-off = I tetti di Proposte sono spenti (override locale): resta posto.
policy-room-left = Resta posto per una Proposta nuova.
policy-room-none = Non resta posto: l'idea resta in Inbox (protocollo §1.2 punto 6).
policy-unknown-project = progetto sconosciuto: { $project }
policy-weekly-unavailable = conteggio settimanale non disponibile: { $reason }
