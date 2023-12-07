use rustrict::CensorStr;
use serde::Serialize;
use thiserror::Error;

use crate::{clashmap::ClashMap, clashset::ClashSet};

use super::watcher::WatcherId;

#[derive(Debug, Default, Clone)]
pub struct Names {
    mapping: ClashMap<WatcherId, String>,
    existing_names: ClashSet<String>,
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
        self.mapping.get(id)
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
        match self.mapping.insert_if_vacant(id, name.to_owned()) {
            Some(_) => {
                self.existing_names.remove(name);
                Err(NamesError::Assigned)
            }
            None => Ok(name.to_owned()),
        }
    }
}
