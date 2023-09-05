use dashmap::{mapref::entry::Entry, DashMap, DashSet};
use serde::Serialize;
use thiserror::Error;

use super::watcher::WatcherId;

#[derive(Debug, Default, Clone)]
pub struct Names {
    mapping: DashMap<WatcherId, String>,
    existing_names: DashSet<String>,
}

#[derive(Error, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamesError {
    #[error("name already in-use")]
    Used,
    #[error("player has an existing name")]
    Assigned,
}

impl actix_web::error::ResponseError for NamesError {}

impl Names {
    pub fn get_name(&self, id: &WatcherId) -> Option<String> {
        self.mapping.get(id).as_ref().map(|x| x.value().to_owned())
    }

    pub fn set_name(&self, id: WatcherId, name: String) -> Result<String, NamesError> {
        if !self.existing_names.insert(name.clone()) {
            return Err(NamesError::Used);
        }
        match self.mapping.entry(id) {
            Entry::Occupied(_) => Err(NamesError::Assigned),
            Entry::Vacant(v) => {
                v.insert(name.clone());
                Ok(name)
            }
        }
    }
}
