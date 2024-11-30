use std::collections::{hash_map::Entry, HashMap, HashSet};

use rustrict::CensorStr;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::watcher::Id;

#[derive(Deserialize)]
struct NamesSerde {
    mapping: HashMap<Id, String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(from = "NamesSerde")]
pub struct Names {
    mapping: HashMap<Id, String>,

    #[serde(skip_serializing)]
    reverse_mapping: HashMap<String, Id>,
    #[serde(skip_serializing)]
    existing: HashSet<String>,
}

impl From<NamesSerde> for Names {
    fn from(serde: NamesSerde) -> Self {
        let NamesSerde { mapping } = serde;
        let mut reverse_mapping = HashMap::new();
        let mut existing = HashSet::new();
        for (id, name) in mapping.iter() {
            reverse_mapping.insert(name.to_owned(), *id);
            existing.insert(name.to_owned());
        }
        Self {
            mapping,
            reverse_mapping,
            existing,
        }
    }
}

#[derive(Error, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    #[error("name already in-use")]
    Used,
    #[error("player has an existing name")]
    Assigned,
    #[error("name cannot be empty")]
    Empty,
    #[error("name is inappropriate")]
    Sinful,
    #[error("name is too long")]
    TooLong,
}

impl Names {
    pub fn get_name(&self, id: &Id) -> Option<String> {
        self.mapping.get(id).map(|s| s.to_owned())
    }

    pub fn set_name(&mut self, id: Id, name: &str) -> Result<String, Error> {
        if name.len() > 30 {
            return Err(Error::TooLong);
        }
        let name = rustrict::trim_whitespace(name);
        if name.is_empty() {
            return Err(Error::Empty);
        }
        if name.is_inappropriate() {
            return Err(Error::Sinful);
        }
        if !self.existing.insert(name.to_owned()) {
            return Err(Error::Used);
        }
        match self.mapping.entry(id) {
            Entry::Occupied(_) => Err(Error::Assigned),
            Entry::Vacant(v) => {
                v.insert(name.to_owned());
                self.reverse_mapping.insert(name.to_owned(), id);
                Ok(name.to_owned())
            }
        }
    }

    pub fn get_id(&self, name: &str) -> Option<Id> {
        self.reverse_mapping.get(name).copied()
    }
}
