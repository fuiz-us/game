use dashmap::DashMap;
use derive_where::derive_where;
use uuid::Uuid;

use super::session::Tunnel;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Watcher {
    pub id: Uuid,
    pub kind: WatcherType,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WatcherType {
    Host,
    Player(String),
}

#[derive_where(Default)]
pub struct Watchers<T: Tunnel> {
    host_watchers: DashMap<Watcher, T>,
    player_watchers: DashMap<Watcher, T>,
}

impl<T: Tunnel> Watchers<T> {
    pub fn iter(
        &self,
    ) -> std::iter::Chain<dashmap::iter::Iter<'_, Watcher, T>, dashmap::iter::Iter<'_, Watcher, T>>
    {
        self.host_watchers.iter().chain(self.player_watchers.iter())
    }

    pub fn players_iter(&self) -> dashmap::iter::Iter<'_, Watcher, T> {
        self.player_watchers.iter()
    }

    pub fn _hosts_iter(&self) -> dashmap::iter::Iter<'_, Watcher, T> {
        self.host_watchers.iter()
    }

    pub fn _players_count(&self) -> usize {
        self.player_watchers.len()
    }

    pub fn hosts_count(&self) -> usize {
        self.host_watchers.len()
    }

    pub fn add_watcher(&self, watcher: Watcher, session: T) {
        match watcher.kind {
            WatcherType::Host => self.host_watchers.insert(watcher, session),
            WatcherType::Player(_) => self.player_watchers.insert(watcher, session),
        };
    }

    pub fn remove_watcher(&self, watcher: &Watcher) {
        match watcher.kind {
            WatcherType::Host => self.host_watchers.remove(watcher),
            WatcherType::Player(_) => self.player_watchers.remove(watcher),
        };
    }
}
