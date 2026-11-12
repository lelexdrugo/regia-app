//! #445: the signature of a comment, `comment` and `declare` in the binary, against the prototype.
//!
//! - **The signature** (M1, M2 of the conformance study): the vectors of the prototype's
//!   `test_firma_commenti.py`, byte for byte, on the product's one signer and one reader.

use regia_core::{SignatureRole, sign, signature_role};

/// The session of the prototype's `TestFirma`.
const SESSION: &str = "sess-orchestratore";

fn marker(role: &str, session: &str) -> String {
    format!("<!-- regia:autore ruolo={role} sessione={session} -->")
}

/// The vectors of `TestRuoloFirma`, through the product's one reader of the marker (M2). The prototype's
/// twelfth, a body that is `None`, has no counterpart in a `&str`.
#[test]
fn the_marker_reads_as_the_prototype_reads_it() {
    let cases: [(&str, Option<&str>); 11] = [
        ("x\n<!-- regia:autore ruolo=worker sessione=a -->", Some("worker")),
        ("x\n<!-- regia:autore ruolo=titolare sessione=- -->\n\n  \n", Some("titolare")),
        ("x\n<!--regia:autore  ruolo=chat-->", Some("chat")),
        ("x\n<!-- regia:autore ruolo=orchestratore -->", Some("orchestratore")),
        ("x", None),
        ("", None),
        ("<!-- regia:autore ruolo=worker sessione=a -->\nsegue testo", None),
        ("x\n<!-- regia:autore ruolo= sessione=a -->", None),
        ("x\n<!-- regia:autore sessione=a -->", None),
        ("x\n<!-- regia:autore ruolo=worker sessione=a", None),
        ("x <!-- regia:autore ruolo=worker -->", None),
    ];
    for (body, role) in cases {
        assert_eq!(signature_role(body).as_deref(), role, "{body:?}");
    }
}

/// The bodies of `TestFirma`, from the product's signer: each role's visible line and marker, the session
/// as `-` without one, a body already signed left as it is, a marker quoted in the middle signed after it.
#[test]
fn the_signature_is_the_prototype_s_byte_for_byte() {
    use SignatureRole::{Chat, Orchestrator, Owner, Worker};
    assert_eq!(sign("ciao", Chat, Some(SESSION)), format!("ciao\n\n— regia · chat\n{}", marker("chat", SESSION)));
    assert_eq!(sign("ciao", Worker, Some(SESSION)), format!("ciao\n\n— regia · worker\n{}", marker("worker", SESSION)));
    assert_eq!(
        sign("per #7 scelgo A", Owner, Some(SESSION)),
        format!("per #7 scelgo A\n\n— il titolare, via chat\n{}", marker("titolare", SESSION))
    );
    assert_eq!(sign("ciao", Orchestrator, Some(SESSION)), format!("ciao\n\n— regia · orchestratore\n{}", marker("orchestratore", SESSION)));
    assert_eq!(sign("x", Chat, None).lines().last(), Some(marker("chat", "-").as_str()));
    let already = "ciao\n\n— regia · worker\n<!-- regia:autore ruolo=worker sessione=abc -->\n";
    assert_eq!(sign(already, Chat, Some(SESSION)), already);
    let quoted = sign("il marcatore è `<!-- regia:autore ruolo=x -->`\naltro testo", Chat, Some(SESSION));
    assert_eq!((quoted.matches("regia:autore").count(), signature_role(&quoted).as_deref()), (2, Some("chat")));
    assert!(sign("dal file\n", Worker, Some(SESSION)).starts_with("dal file\n\n— regia · worker\n<!-- regia:autore ruolo=worker"));
    for role in SignatureRole::ALL {
        let word = role.word();
        assert_eq!(signature_role(&sign("x", role, Some(SESSION))).as_deref(), Some(word));
    }
}
