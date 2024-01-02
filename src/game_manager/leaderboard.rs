use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

use dashmap::{mapref::entry::Entry, DashMap};
use itertools::Itertools;
use serde::Serialize;

use super::watcher::Id;

type Scores = Vec<(Id, u64)>;
type Positions = HashMap<Id, usize>;

#[derive(Debug, Default)]
pub struct Leaderboard {
    mapping: DashMap<Id, u64>,

    invalidated: Arc<AtomicBool>,
    scores_descending: Arc<Mutex<Scores>>,
    position_mapping: Arc<Mutex<Positions>>,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub struct ScoreMessage {
    pub points: u64,
    pub position: usize,
}

#[derive(Debug, Serialize, Clone)]
pub struct Message {
    pub exact_count: usize,
    pub points: Vec<(String, u64)>,
}

impl Leaderboard {
    pub fn invalidate_cache(&self) {
        self.invalidated.store(true, Ordering::SeqCst);
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

        self.invalidated
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn add_score(&self, player: Id, score: u64) {
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

    pub fn _remove_player(&self, player: Id) {
        self.mapping.remove(&player);
    }

    pub fn get_scores_truncated(&self) -> (usize, Scores) {
        if self.invalidated.load(Ordering::SeqCst) {
            self.update_cache();
        }

        let scores = self
            .scores_descending
            .lock()
            .expect("Program must not panic");

        (scores.len(), scores.iter().take(50).copied().collect_vec())
    }

    pub fn score(&self, watcher_id: Id) -> Option<ScoreMessage> {
        if self.invalidated.load(Ordering::SeqCst) {
            self.update_cache();
        }

        let positions = self
            .position_mapping
            .lock()
            .expect("Program must not panic");

        positions.get(&watcher_id).map(|&position| ScoreMessage {
            points: self.mapping.get(&watcher_id).map_or(0, |x| *x),
            position,
        })
    }
}
