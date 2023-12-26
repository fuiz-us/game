use rustrict::CensorStr;
use serde::Serialize;
use thiserror::Error;

use crate::{clashmap::ClashMap, clashset::ClashSet};

use super::watcher::Id;

#[derive(Debug, Default, Clone)]
pub struct Names {
    mapping: ClashMap<Id, String>,
    existing_names: ClashSet<String>,
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
        if !self.existing_names.insert(name.to_owned()) {
            return Err(Error::Used);
        }
        match self.mapping.insert_if_vacant(id, name.to_owned()) {
            Some(_) => {
                self.existing_names.remove(name);
                Err(Error::Assigned)
            }
            None => Ok(name.to_owned()),
        }
    }
}
