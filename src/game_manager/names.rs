use dashmap::{mapref::entry::Entry, DashMap};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Default, Clone)]
pub struct Names(DashMap<Uuid, String>);

#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamesError {
    #[error("name already in-use")]
    Used,
}

impl actix_web::error::ResponseError for NamesError {}

impl Names {
    pub fn get_name(&self, id: &Uuid) -> Option<String> {
        self.0.get(id).as_ref().map(|x| x.value().to_owned())
    }

    pub fn set_name(&self, id: Uuid, name: String) -> Result<(), NamesError> {
        match self.0.entry(id) {
            Entry::Occupied(_) => Err(NamesError::Used),
            Entry::Vacant(v) => {
                v.insert(name);
                Ok(())
            }
        }
    }
}
