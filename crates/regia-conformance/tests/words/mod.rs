//! The prototype's sentences in the product's words, for the tests that compare the two: the reasons of
//! the dossier's block (`checkpoint`, `step finish`) and those of a lock or a lease. A sentence the table
//! does not know passes as it is, and the comparison then shows it.

#![allow(dead_code)]

/// The prototype's reason in the product's words: each of its sentences, and the warnings it joins with
/// `; `, has one in `regia-git`; what git said passes as it is.
pub fn in_the_product_s_words(reason: &str) -> String {
    const SENTENCES: [(&str, &str); 14] = [
        ("cartella del progetto non letta: ", "the project's folder was not read: "),
        ("la cartella del progetto non esiste", "the project's folder does not exist"),
        ("la cartella non è la radice di un repo git", "the folder is not the root of a git repo"),
        ("': deve essere relativo alla cartella del progetto", "': must be relative to the project's folder"),
        ("': esce dalla cartella del progetto", "': leaves the project's folder"),
        ("': nomina la cartella intera o .git", "': names the whole folder or .git"),
        ("HEAD staccato: nessun branch su cui committare", "detached HEAD: no branch to commit on"),
        ("RegiaError: git status: ", "git status: "),
        ("nessun remoto configurato", "no remote configured"),
        ("upstream senza remoto o riferimento leggibili", "upstream with no readable remote or ref"),
        ("fetch non riuscito: ", "fetch failed: "),
        ("fast-forward non riuscito: ", "fast-forward failed: "),
        (" (divergenza: la risolve il titolare)", " (divergence: the owner resolves it)"),
        (" non ha un upstream", " has no upstream"),
    ];
    let mut out = SENTENCES.iter().fold(reason.to_owned(), |text, (python, product)| text.replace(python, product));
    out = out.replace("il branch ", "the branch ");
    if let Some(project) = between(&out, "progetto '", "' sconosciuto") {
        out = out.replace(&format!("progetto '{project}' sconosciuto"), &format!("project '{project}' is unknown"));
    }
    if let Some(project) = between(&out, "il registro di '", "' non ha la cartella") {
        out = out.replace(&format!("il registro di '{project}' non ha la cartella"), &format!("the registry of '{project}' has no folder"));
    }
    if let Some(count) = between(&out, "ritentato: ", " commit già fatti") {
        let product = if count == "1" { "retried: 1 commit already made".to_owned() } else { format!("retried: {count} commits already made") };
        out = out.replace(&format!("ritentato: {count} commit già fatti"), &product);
    }
    out
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
    let from = text.find(start)? + start.len();
    let to = from + text[from..].find(end)?;
    Some(&text[from..to])
}

/// The outcome the prototype's text line names: `dossier_riga`.
pub fn outcome_of_line(text: &str) -> &'static str {
    let line = text.split_once(": ").map_or(text, |(_, rest)| rest);
    [("pushato", "pushed"), ("commit locale", "local_commit"), ("niente da committare", "nothing"), ("nessun remoto", "no_remote"), ("non disponibile", "unavailable")]
        .iter()
        .find(|(python, _)| line.starts_with(python))
        .map_or("(no outcome)", |(_, product)| product)
}

/// The reason of a refusal of a lock, a lease or a step in the product's words: who holds it and until
/// when, that it is someone else's, that there is none, that it is contended, and the tail of D3.
pub fn state_reason_in_the_product_s_words(reason: &str) -> String {
    let reason = reason.replace(": scaduto prima di scrivere il cursore del passo", ": it expired before the step's cursor was written");
    for name in ["lock", "lease"] {
        if let Some(rest) = reason.strip_prefix(&format!("{name} di ")) {
            if let Some((holder, until)) = rest.split_once(", valido fino a ") {
                return format!("{name} of {holder}, valid until {until}");
            }
            if let Some((holder, tail)) = rest.split_once(", non tuo") {
                return format!("{name} of {holder}, not yours{tail}");
            }
        }
        if let Some(tail) = reason.strip_prefix(&format!("{name} assente")) {
            return format!("no {name}{tail}");
        }
        if reason == format!("{name} in contesa, riprova") {
            return format!("{name} contended, try again");
        }
    }
    reason
}
