use std::{fmt::Display, str::FromStr};

use enum_map::{Enum, EnumMap};
use itertools::Itertools;
use kinded::Kinded;
use serde::Serialize;
use serde_with::{DeserializeFromStr, SerializeDisplay};
use thiserror::Error;
use uuid::Uuid;

use crate::{clashmap::ClashMap, clashset::ClashSet};

use super::{session::Tunnel, UpdateMessage};

#[derive(
    Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, DeserializeFromStr, SerializeDisplay,
)]
pub struct Id(Uuid);

impl Id {
    pub fn get_seed(&self) -> u64 {
        self.0.as_u64_pair().0
    }

    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Id {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::from_str(s)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Kinded)]
#[kinded(derive(Hash, Enum))]
pub enum Value {
    Unassigned,
    Host,
    Player(String),
}

#[derive_where::derive_where(Default)]
pub struct Watchers<T: Tunnel> {
    sessions: ClashMap<Id, T>,
    watchers: ClashMap<Id, Value>,
    reverse_watchers: EnumMap<ValueKind, ClashSet<Id>>,
}

const MAX_PLAYERS: usize = crate::CONFIG.fuiz.max_player_count.unsigned_abs() as usize;

#[derive(Error, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    #[error("maximum number of players reached")]
    MaximumPlayers,
}

impl actix_web::error::ResponseError for Error {}

impl<T: Tunnel> Watchers<T> {
    pub fn vec(&self) -> Vec<(Id, T, Value)> {
        self.reverse_watchers
            .values()
            .flat_map(|x| x.iter())
            .filter_map(|x| match (self.sessions.get(&x), self.watchers.get(&x)) {
                (Some(t), Some(v)) => Some((x, t, v)),
                _ => None,
            })
            .collect_vec()
    }

    pub fn specific_vec(&self, filter: ValueKind) -> Vec<(Id, T, Value)> {
        self.reverse_watchers[filter]
            .iter()
            .filter_map(|x| match (self.sessions.get(&x), self.watchers.get(&x)) {
                (Some(t), Some(v)) => Some((x, t, v)),
                _ => None,
            })
            .collect_vec()
    }

    pub fn specific_count(&self, filter: ValueKind) -> usize {
        self.reverse_watchers[filter].len()
    }

    pub fn add_watcher(
        &self,
        watcher_id: Id,
        watcher_value: Value,
        session: T,
    ) -> Result<(), Error> {
        let kind = watcher_value.kind();

        if self.sessions.len() >= MAX_PLAYERS {
            return Err(Error::MaximumPlayers);
        }

        if let Some(x) = self.sessions.insert(watcher_id, session) {
            x.close();
        }

        self.watchers.insert(watcher_id, watcher_value);
        self.reverse_watchers[kind].insert(watcher_id);

        Ok(())
    }

    pub fn update_watcher_value(&self, watcher_id: Id, watcher_value: Value) {
        let old_kind = match self.watchers.get(&watcher_id) {
            Some(v) => v.kind(),
            _ => return,
        };
        let new_kind = watcher_value.kind();
        if old_kind != new_kind {
            self.reverse_watchers[old_kind].remove(&watcher_id);
            self.reverse_watchers[new_kind].insert(watcher_id);
        }
        self.watchers.insert(watcher_id, watcher_value);
    }

    pub fn update_watcher_session(&self, watcher_id: Id, session: T) {
        self.sessions.insert(watcher_id, session);
    }

    pub fn get_watcher_value(&self, watcher_id: Id) -> Option<Value> {
        self.watchers.get(&watcher_id)
    }

    pub fn has_watcher(&self, watcher_id: Id) -> bool {
        self.watchers.contains_key(&watcher_id)
    }

    pub fn reserve_watcher(&self, watcher_id: Id, watcher_value: Value) {
        let kind = watcher_value.kind();
        self.watchers.insert(watcher_id, watcher_value);
        self.reverse_watchers[kind].insert(watcher_id);
    }

    pub fn remove_watcher_session(&self, watcher_id: &Id) {
        if let Some((_, x)) = self.sessions.remove(watcher_id) {
            x.close();
        }
    }

    pub fn send_message(&self, message: &UpdateMessage, watcher_id: Id) {
        let Some(session) = self.sessions.get(&watcher_id) else {
            return;
        };

        session.send_message(message);
    }
}
