### `plan` (#444): the tree of the open tasks, the line before the diagram, the warnings and the errors. The JSON is
### the contract, and the diagram in it is data in the prototype's words (`formats/plan_v1.toml` of `regia-core`):
### these are the words around it. Separators, brackets and arrows stay in the code, since a message loses its blanks.

plan-nothing = (nessun task)
plan-outside = un passo fuori dal perimetro:
plan-cycle = in ciclo
plan-unblocks = sblocca { $unblocks }
plan-unblocks-total = ({ $total } con Inbox e Proposte)
plan-chain = catena { $chain }
plan-waits = aspetta { $numbers }
plan-note-inbox = { $count } Inbox senza archi:
plan-note-nested = archi fra padre e figlio, non disegnati:
plan-note-truncated = tagliati { $count } aperti oltre il tetto
plan-note-lost = { $lost } archi persi:
plan-note-hint = per il resto usa { $option } su un sotto-albero
plan-main-line = linea principale: { $line }
plan-markdown-head = Piano dal tracker al { $date }
plan-markdown-legend = ✋ aspetta il titolare, frecce spesse = linea principale
plan-children-waiting = Figli che aspettano il padre con un arco che chiuderebbe un ciclo, e non conta: { $edges }
plan-max-nodes = { $option } deve essere almeno 1
plan-root-missing = issue #{ $number } non trovata nel tracker
plan-project-missing = progetto «{ $project }» senza task nel tracker
