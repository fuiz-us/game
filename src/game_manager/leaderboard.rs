use dashmap::{mapref::entry::Entry, DashMap};
use itertools::Itertools;
use serde::Serialize;

use super::watcher::WatcherId;

#[derive(Debug, Default)]
pub struct Leaderboard {
    mapping: DashMap<WatcherId, u64>,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub struct ScoreMessage {
    points: u64,
    position: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct LeaderboardMessage {
    pub exact_count: usize,
    pub points: Vec<(String, u64)>,
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

    pub fn get_scores_descending_truncated(&self) -> (usize, Vec<(WatcherId, u64)>) {
        // This is split into two in order to release the lock on mapping
        let values = self
            .mapping
            .iter()
            .map(|x| (x.key().to_owned(), x.value().to_owned()))
            .collect_vec();

        (
            values.len(),
            values
                .into_iter()
                .sorted_by_key(|(_, v)| *v)
                .rev()
                .take(20)
                .collect_vec(),
        )
    }

    pub fn score(&self, watcher_id: WatcherId) -> Option<ScoreMessage> {
        let scores_descending = self.get_scores_descending();

        scores_descending
            .into_iter()
            .find_position(|(w, _)| *w == watcher_id)
            .map(|(position, (_, points))| ScoreMessage { points, position })
    }
}
