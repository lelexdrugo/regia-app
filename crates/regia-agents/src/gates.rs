//! The gates: what always goes through a decision of the owner. The product's list is compiled in
//! ([`gates.toml`](../policy/gates.toml)); a user setting can add a gate and never remove one of it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Gate {
    pub id: String,
    pub text: String,
    /// From the product, which no setting removes, or added by the user.
    pub product: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GateFile {
    always_decision: BTreeMap<String, String>,
    never: BTreeMap<String, String>,
    not_gates: BTreeMap<String, String>,
}

/// The gates section of the user's settings.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateSettings {
    /// Gates of the user's own, by id, with their text.
    #[serde(default)]
    add: BTreeMap<String, String>,
    /// Present only so that an attempt is refused by name, not ignored.
    #[serde(default)]
    remove: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GateError {
    #[error("the gate {0} is the product's: a setting cannot remove it")]
    ProductGate(String),
    #[error("there is no gate {0} to remove")]
    Unknown(String),
    #[error("the gate id {0} is not lowercase words with _")]
    InvalidId(String),
}

#[derive(Debug, Clone)]
pub struct Gates {
    always: Vec<Gate>,
    never: Vec<Gate>,
    not_gates: Vec<Gate>,
}

fn list(entries: BTreeMap<String, String>, product: bool) -> Vec<Gate> {
    entries.into_iter().map(|(id, text)| Gate { id, text, product }).collect()
}

impl Gates {
    pub fn product() -> Gates {
        let file: GateFile = toml::from_str(include_str!("../policy/gates.toml")).expect("the compiled gates are valid");
        Gates { always: list(file.always_decision, true), never: list(file.never, true), not_gates: list(file.not_gates, true) }
    }

    /// The product's gates and the user's. Removing a product gate is an error, never a no-op.
    pub fn with_settings(mut self, settings: &GateSettings) -> Result<Gates, GateError> {
        if let Some(id) = settings.remove.first() {
            let known = self.always.iter().chain(&self.never).any(|gate| &gate.id == id);
            return Err(if known { GateError::ProductGate(id.clone()) } else { GateError::Unknown(id.clone()) });
        }
        for (id, text) in &settings.add {
            let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !valid {
                return Err(GateError::InvalidId(id.clone()));
            }
            // An id the product has already stays the product's.
            if !self.always.iter().any(|gate| &gate.id == id) {
                self.always.push(Gate { id: id.clone(), text: text.clone(), product: false });
            }
        }
        Ok(self)
    }

    /// What always goes through a decision.
    pub fn always_decision(&self) -> &[Gate] {
        &self.always
    }

    /// What is never done.
    pub fn never(&self) -> &[Gate] {
        &self.never
    }

    pub fn not_gates(&self) -> &[Gate] {
        &self.not_gates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(text: &str) -> GateSettings {
        toml::from_str(text).unwrap()
    }

    /// The prototype's `policies/gates.yaml`, entry by entry.
    #[test]
    fn the_product_gates_are_the_prototype_ones() {
        let gates = Gates::product();
        let ids: Vec<&str> = gates.always_decision().iter().map(|gate| gate.id.as_str()).collect();
        assert_eq!(ids.len(), 8);
        for id in ["message_people", "publish", "submit", "push_shared_repo", "deploy_release_visibility", "spend", "delete_unrecoverable", "change_contract"] {
            assert!(ids.contains(&id), "{id}");
        }
        assert_eq!(gates.never().len(), 2);
        assert_eq!(gates.not_gates().len(), 1);
        assert!(gates.always_decision().iter().chain(gates.never()).all(|gate| gate.product));
    }

    /// Acceptance 3 of #460: a setting adds a gate and never takes one of the product's away.
    #[test]
    fn a_setting_adds_a_gate_and_never_removes_a_product_one() {
        let added = Gates::product().with_settings(&settings("[add]\nprint_job = \"printing on the lab printer\"\n")).unwrap();
        assert_eq!(added.always_decision().len(), 9);
        assert!(added.always_decision().iter().any(|gate| gate.id == "print_job" && !gate.product));

        for id in ["publish", "spend", "public_links"] {
            let refused = Gates::product().with_settings(&settings(&format!("remove = [\"{id}\"]\n")));
            assert_eq!(refused.unwrap_err(), GateError::ProductGate(id.to_owned()));
        }
        assert!(matches!(Gates::product().with_settings(&settings("remove = [\"x\"]")), Err(GateError::Unknown(_))));
        // Adding a product id again changes nothing: it is still the product's text and kind.
        let same = Gates::product().with_settings(&settings("[add]\npublish = \"nothing\"\n")).unwrap();
        let publish = same.always_decision().iter().find(|gate| gate.id == "publish").unwrap();
        assert!(publish.product && publish.text != "nothing");
        assert_eq!(same.always_decision().len(), 8);
        assert!(Gates::product().with_settings(&settings("[add]\n\"Bad Id\" = \"x\"\n")).is_err());
        // No other key can drop a gate either.
        assert!(toml::from_str::<GateSettings>("disable = [\"publish\"]").is_err());
    }
}
