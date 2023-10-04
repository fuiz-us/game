use std::str::FromStr;

use dashmap::{DashMap, DashSet};
use derive_where::derive_where;
use enum_map::{Enum, EnumMap};
use itertools::Itertools;
use kinded::Kinded;
use uuid::Uuid;

use super::session::Tunnel;

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WatcherId(Uuid);

impl WatcherId {
    pub fn get_seed(&self) -> u64 {
        self.0.as_u64_pair().0
    }
}

impl Default for WatcherId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

impl ToString for WatcherId {
    fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl FromStr for WatcherId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::from_str(s)?))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Kinded)]
#[kinded(derive(Hash, Enum))]
pub enum WatcherValue {
    Unassigned,
    Host,
    Player(String),
}

#[derive_where(Default)]
pub struct Watchers<T: Tunnel> {
    sessions: DashMap<WatcherId, T>,
    watchers: DashMap<WatcherId, WatcherValue>,
    reverse_watchers: EnumMap<WatcherValueKind, DashSet<WatcherId>>,
}

impl<T: Tunnel> Watchers<T> {
    pub fn vec(
        &self,
    ) -> Vec<(
        WatcherId,
        dashmap::mapref::one::Ref<'_, WatcherId, T>,
        WatcherValue,
    )> {
        self.reverse_watchers
            .values()
            .flat_map(|x| x.iter())
            .flat_map(|x| match (self.sessions.get(&x), self.watchers.get(&x)) {
                (Some(t), Some(v)) => Some((x.to_owned(), t, v.value().to_owned())),
                _ => None,
            })
            .collect_vec()
    }

    pub fn specific_vec(
        &self,
        filter: WatcherValueKind,
    ) -> Vec<(
        WatcherId,
        dashmap::mapref::one::Ref<'_, WatcherId, T>,
        WatcherValue,
    )> {
        self.reverse_watchers[filter]
            .iter()
            .flat_map(|x| match (self.sessions.get(&x), self.watchers.get(&x)) {
                (Some(t), Some(v)) => Some((x.to_owned(), t, v.value().to_owned())),
                _ => None,
            })
            .collect_vec()
    }

    pub fn specific_count(&self, filter: WatcherValueKind) -> usize {
        self.reverse_watchers[filter].len()
    }

    pub fn add_watcher(&self, watcher_id: WatcherId, watcher_value: WatcherValue, session: T) {
        let kind = watcher_value.kind();
        self.sessions.insert(watcher_id, session);
        self.watchers.insert(watcher_id, watcher_value);
        self.reverse_watchers[kind].insert(watcher_id);
    }

    pub fn update_watcher_value(&self, watcher_id: WatcherId, watcher_value: WatcherValue) {
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

    pub fn update_watcher_session(&self, watcher_id: WatcherId, session: T) {
        self.sessions.insert(watcher_id, session);
    }

    pub fn get_watcher_value(&self, watcher_id: WatcherId) -> Option<WatcherValue> {
        self.watchers.get(&watcher_id).map(|x| x.value().to_owned())
    }

    pub fn has_watcher(&self, watcher_id: WatcherId) -> bool {
        self.watchers.get(&watcher_id).is_some()
    }

    pub fn reserve_watcher(&self, watcher_id: WatcherId, watcher_value: WatcherValue) {
        let kind = watcher_value.kind();
        self.watchers.insert(watcher_id, watcher_value);
        self.reverse_watchers[kind].insert(watcher_id);
    }

    pub async fn remove_watcher_session(&self, watcher_id: &WatcherId) {
        if let Some((_, x)) = self.sessions.remove(watcher_id) {
            x.close().await;
        }
    }

    pub async fn send(&self, message: &str, watcher_id: WatcherId) {
        let Some(session) = self.sessions.get(&watcher_id) else {
            return;
        };

        if session.send(message).await.is_err() {
            self.sessions.remove(&watcher_id);
        }
    }
}
