use dashmap::{mapref::entry::Entry, DashMap, DashSet};
use rustrict::CensorStr;
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
    #[error("name cannot be empty")]
    Empty,
    #[error("name is inappropriate")]
    Sinful,
    #[error("name is too long")]
    TooLong,
}

impl actix_web::error::ResponseError for NamesError {}

impl Names {
    pub fn get_name(&self, id: &WatcherId) -> Option<String> {
        self.mapping.get(id).as_ref().map(|x| x.value().to_owned())
    }

    pub fn set_name(&self, id: WatcherId, name: String) -> Result<String, NamesError> {
        if name.len() > 30 {
            return Err(NamesError::TooLong);
        }
        let name = rustrict::trim_whitespace(&name);
        if name.is_empty() {
            return Err(NamesError::Empty);
        }
        if name.is_inappropriate() {
            return Err(NamesError::Sinful);
        }
        if !self.existing_names.insert(name.to_owned()) {
            return Err(NamesError::Used);
        }
        match self.mapping.entry(id) {
            Entry::Occupied(_) => Err(NamesError::Assigned),
            Entry::Vacant(v) => {
                v.insert(name.to_owned());
                Ok(name.to_owned())
            }
        }
    }
}
