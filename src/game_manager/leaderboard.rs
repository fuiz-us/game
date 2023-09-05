use dashmap::{mapref::entry::Entry, DashMap};
use itertools::Itertools;

use super::watcher::WatcherId;

#[derive(Debug, Default)]
pub struct Leaderboard {
    mapping: DashMap<WatcherId, u64>,
}

impl Leaderboard {
    pub fn add_score(&self, player: WatcherId, score: u64) {
        match self.mapping.entry(player) {
            Entry::Occupied(o) => {
                let score = o.get().saturating_add(score);
                o.replace_entry(score);
            }
            Entry::Vacant(v) => {
                v.insert(score);
            }
        }
    }

    pub fn _remove_player(&self, player: WatcherId) {
        self.mapping.remove(&player);
    }

    pub fn get_scores_descending(&self) -> Vec<(WatcherId, u64)> {
        // This is split into two in order to release the lock on mapping
        let values = self
            .mapping
            .iter()
            .map(|x| (x.key().to_owned(), x.value().to_owned()))
            .collect_vec();
        values
            .into_iter()
            .sorted_by_key(|(_, v)| *v)
            .rev()
            .collect_vec()
    }
}
