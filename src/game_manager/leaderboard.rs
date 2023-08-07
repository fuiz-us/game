use dashmap::{mapref::entry::Entry, DashMap};
use itertools::Itertools;
use uuid::Uuid;

#[derive(Debug, Default)]
pub struct Leaderboard {
    mapping: DashMap<Uuid, u64>,
}

impl Leaderboard {
    pub fn add_score(&self, player: Uuid, score: u64) {
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

    pub fn _remove_player(&self, player: Uuid) {
        self.mapping.remove(&player);
    }

    pub fn get_scores_descending(&self) -> Vec<(Uuid, u64)> {
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
