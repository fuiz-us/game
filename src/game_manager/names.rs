use rustrict::CensorStr;
use serde::Serialize;
use thiserror::Error;

use crate::{clashmap::ClashMap, clashset::ClashSet};

use super::watcher::Id;

#[derive(Debug, Default, Clone)]
pub struct Names {
    mapping: ClashMap<Id, String>,
    reverse_mapping: ClashMap<String, Id>,
    existing: ClashSet<String>,
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

impl actix_web::error::ResponseError for Error {}

impl Names {
    pub fn get_name(&self, id: &Id) -> Option<String> {
        self.mapping.get(id)
    }

    pub fn set_name(&self, id: Id, name: &str) -> Result<String, Error> {
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
        if self.mapping.insert_if_vacant(id, name.to_owned()).is_some() {
            self.existing.remove(name);
            Err(Error::Assigned)
        } else {
            self.reverse_mapping.insert(name.to_owned(), id);
            Ok(name.to_owned())
        }
    }

    pub fn get_id(&self, name: &str) -> Option<Id> {
        self.reverse_mapping.get(name)
    }
}
