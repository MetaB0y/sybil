//! Resolution templates: reusable policy definitions referenced by markets.
//!
//! A template is a (string id, policy) pair. Markets carry a `String` template
//! id in their metadata; the sequencer resolves the id against a
//! `TemplateRegistry` at resolution time. Templates are system-wired at
//! startup and deliberately not persisted in redb — when user-defined
//! templates land, this will become redb-backed (see design doc).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::policy::ResolutionPolicy;

/// String identifier for a template (e.g. `"admin_immediate"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TemplateId(pub String);

impl From<&str> for TemplateId {
    fn from(value: &str) -> Self {
        TemplateId(value.to_string())
    }
}

impl From<String> for TemplateId {
    fn from(value: String) -> Self {
        TemplateId(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolutionTemplate {
    pub id: TemplateId,
    pub policy: ResolutionPolicy,
}

/// In-memory store of available templates.
#[derive(Clone, Debug, Default)]
pub struct TemplateRegistry {
    map: HashMap<TemplateId, ResolutionTemplate>,
}

impl TemplateRegistry {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn install(&mut self, template: ResolutionTemplate) {
        self.map.insert(template.id.clone(), template);
    }

    pub fn get(&self, id: &TemplateId) -> Option<&ResolutionTemplate> {
        self.map.get(id)
    }

    pub fn get_str(&self, id: &str) -> Option<&ResolutionTemplate> {
        self.map.get(&TemplateId(id.to_string()))
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&TemplateId, &ResolutionTemplate)> {
        self.map.iter()
    }
}
