use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc, Mutex},
};

use dashmap::{mapref::entry::Entry, DashMap};
use itertools::Itertools;
use serde::Serialize;

use super::watcher::WatcherId;

type Scores = Vec<(WatcherId, u64)>;
type Positions = HashMap<WatcherId, usize>;

#[derive(Debug, Default)]
pub struct Leaderboard {
    mapping: DashMap<WatcherId, u64>,

    invalidated: Arc<AtomicBool>,
    scores_descending: Arc<Mutex<Scores>>,
    position_mapping: Arc<Mutex<Positions>>,
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
    pub fn invalidate_cache(&self) {
        self.invalidated.store(true, atomig::Ordering::SeqCst);
    }

    pub fn update_cache(&self) {
        let values = self
            .mapping
            .iter()
            .map(|x| (x.key().to_owned(), x.value().to_owned()))
            .collect_vec();

        let values = values
            .into_iter()
            .sorted_by_key(|(_, v)| *v)
            .rev()
            .collect_vec();

        let positions: Positions = values
            .iter()
            .enumerate()
            .map(|(i, (w, _))| (*w, i))
            .collect();

        let mut x = self
            .scores_descending
            .lock()
            .expect("Program must not panic");

        *x = values;

        let mut x = self
            .position_mapping
            .lock()
            .expect("Program must not panic");

        *x = positions;

        self.invalidated.store(false, atomig::Ordering::SeqCst);
    }

    pub fn add_score(&self, player: WatcherId, score: u64) {
        self.invalidate_cache();

        match self.mapping.entry(player) {
            Entry::Occupied(o) => {
                let score = o.get().saturating_add(score);
                o.replace_entry(score);
            }
            Entry::Vacant(v) => {
                v.insert(score);
            }
        };
    }

    pub fn _remove_player(&self, player: WatcherId) {
        self.mapping.remove(&player);
    }

    pub fn get_scores_truncated(&self) -> (usize, Scores) {
        if self.invalidated.load(atomig::Ordering::SeqCst) {
            self.update_cache();
        }

        let scores = self
            .scores_descending
            .lock()
            .expect("Program must not panic");

        (scores.len(), scores.iter().take(50).cloned().collect_vec())
    }

    pub fn score(&self, watcher_id: WatcherId) -> Option<ScoreMessage> {
        if self.invalidated.load(atomig::Ordering::SeqCst) {
            self.update_cache();
        }

        let positions = self
            .position_mapping
            .lock()
            .expect("Program must not panic");

        positions.get(&watcher_id).map(|&position| ScoreMessage {
            points: self.mapping.get(&watcher_id).map(|x| *x).unwrap_or(0),
            position,
        })
    }
}
