use std::{fmt::Display, str::FromStr};

use enum_map::{Enum, EnumMap};
use itertools::Itertools;
use kinded::Kinded;
use serde::Serialize;
use serde_with::{DeserializeFromStr, SerializeDisplay};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    clashmap::ClashMap,
    clashset::{self, ClashSet},
};

use super::{session::Tunnel, SyncMessage, UpdateMessage};

#[derive(
    Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, DeserializeFromStr, SerializeDisplay,
)]
pub struct Id(Uuid);

impl Id {
    pub fn _get_seed(&self) -> u64 {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Kinded)]
#[kinded(derive(Hash, Enum))]
pub enum Value {
    Unassigned,
    Host,
    Player(PlayerValue),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PlayerValue {
    Individual {
        name: String,
    },
    Team {
        team_name: String,
        individual_name: String,
        team_id: Id,
        player_index_in_team: usize,
    },
}

impl PlayerValue {
    pub fn name(&self) -> &str {
        match self {
            Self::Individual { name } => name,
            Self::Team {
                team_name: _,
                individual_name,
                team_id: _,
                player_index_in_team: _,
            } => individual_name,
        }
    }
}

#[derive_where::derive_where(Default)]
pub struct Watchers<T: Tunnel> {
    sessions: ClashMap<Id, T>,
    mapping: ClashMap<Id, Value>,
    reverse_mapping: EnumMap<ValueKind, ClashSet<Id>>,
}

const MAX_PLAYERS: usize = crate::CONFIG.fuiz.max_player_count.unsigned_abs() as usize;

#[derive(Error, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    #[error("maximum number of players reached")]
    MaximumPlayers,
}

impl actix_web::error::ResponseError for Error {}

impl<T: Tunnel> Watchers<T> {
    pub fn with_host_id(host_id: Id) -> Self {
        Self {
            sessions: ClashMap::default(),
            mapping: {
                let map = ClashMap::default();
                map.insert(host_id, Value::Host);
                map
            },
            reverse_mapping: {
                let map: EnumMap<ValueKind, ClashSet<Id>> = EnumMap::default();
                map[ValueKind::Host].insert(host_id);
                map
            },
        }
    }

    pub fn vec(&self) -> Vec<(Id, T, Value)> {
        self.reverse_mapping
            .values()
            .flat_map(clashset::ClashSet::vec)
            .filter_map(|x| match (self.sessions.get(&x), self.mapping.get(&x)) {
                (Some(t), Some(v)) => Some((x, t, v)),
                _ => None,
            })
            .collect_vec()
    }

    pub fn specific_vec(&self, filter: ValueKind) -> Vec<(Id, T, Value)> {
        self.reverse_mapping[filter]
            .vec()
            .into_iter()
            .filter_map(|x| match (self.sessions.get(&x), self.mapping.get(&x)) {
                (Some(t), Some(v)) => Some((x, t, v)),
                _ => None,
            })
            .collect_vec()
    }

    pub fn specific_count(&self, filter: ValueKind) -> usize {
        self.reverse_mapping[filter].len()
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

        self.mapping.insert(watcher_id, watcher_value);
        self.reverse_mapping[kind].insert(watcher_id);

        Ok(())
    }

    pub fn update_watcher_value(&self, watcher_id: Id, watcher_value: Value) {
        let old_kind = match self.mapping.get(&watcher_id) {
            Some(v) => v.kind(),
            _ => return,
        };
        let new_kind = watcher_value.kind();
        if old_kind != new_kind {
            self.reverse_mapping[old_kind].remove(&watcher_id);
            self.reverse_mapping[new_kind].insert(watcher_id);
        }
        self.mapping.insert(watcher_id, watcher_value);
    }

    pub fn update_watcher_session(&self, watcher_id: Id, session: T) {
        self.sessions.insert(watcher_id, session);
    }

    pub fn get_watcher_value(&self, watcher_id: Id) -> Option<Value> {
        self.mapping.get(&watcher_id)
    }

    pub fn has_watcher(&self, watcher_id: Id) -> bool {
        self.mapping.contains_key(&watcher_id)
    }

    pub fn is_alive(&self, watcher_id: Id) -> bool {
        self.sessions.contains_key(&watcher_id)
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

    pub fn send_state(&self, message: &SyncMessage, watcher_id: Id) {
        let Some(session) = self.sessions.get(&watcher_id) else {
            return;
        };

        session.send_state(message);
    }

    pub fn get_name(&self, watcher_id: Id) -> Option<String> {
        self.get_watcher_value(watcher_id).and_then(|v| match v {
            Value::Player(player_value) => Some(player_value.name().to_owned()),
            _ => None,
        })
    }

    pub fn announce_with<F>(&self, sender: F)
    where
        F: Fn(Id, ValueKind) -> Option<super::UpdateMessage>,
    {
        for (watcher, session, v) in self.vec() {
            let Some(message) = sender(watcher, v.kind()) else {
                continue;
            };

            session.send_message(&message);
        }
    }

    pub fn announce(&self, message: &super::UpdateMessage) {
        self.announce_with(|_, _| Some(message.to_owned()));
    }

    pub fn announce_specific(&self, filter: ValueKind, message: &super::UpdateMessage) {
        for (_, session, _) in self.specific_vec(filter) {
            session.send_message(message);
        }
    }
}
