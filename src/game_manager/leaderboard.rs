use itertools::Itertools;
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{atomic::AtomicUsize, OnceLock},
};

use super::{watcher::Id, TruncatedVec};

#[derive(Debug, Clone)]
pub struct SlideSummary {
    scores_descending: Vec<(Id, u64)>,
    mapping: HashMap<Id, (u64, usize)>,
    points_earned: Vec<(Id, u64)>,
}

#[derive(Debug, Clone)]
pub struct FinalSummary {
    summary_descending: Vec<(Id, Vec<u64>)>,
    mapping: HashMap<Id, Vec<u64>>,
}

#[derive(Debug, Default)]
pub struct Leaderboard {
    slide_summaries: boxcar::Vec<SlideSummary>,
    current_slide: AtomicUsize,
    final_summary: OnceLock<FinalSummary>,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub struct ScoreMessage {
    pub points: u64,
    pub position: usize,
}

impl Leaderboard {
    fn slide(&self) -> usize {
        self.current_slide.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn add_scores(&self, scores: &[(Id, u64)]) {
        let mut summary: HashMap<Id, u64> = self
            .slide_summaries
            .get(self.slide())
            .map(|s| {
                s.mapping
                    .iter()
                    .map(|(id, (points, _))| (*id, *points))
                    .collect()
            })
            .unwrap_or_default();

        for (id, points) in scores {
            *summary.entry(*id).or_default() += points;
        }

        let scores_descending = summary
            .iter()
            .sorted_by_key(|(_, points)| *points)
            .rev()
            .map(|(a, b)| (*a, *b))
            .collect_vec();

        let mapping = scores_descending
            .iter()
            .enumerate()
            .map(|(position, (id, points))| (*id, (*points, position)))
            .collect();

        let i = self.slide_summaries.push(SlideSummary {
            scores_descending,
            mapping,
            points_earned: scores.to_vec(),
        });

        self.current_slide
            .store(i, std::sync::atomic::Ordering::SeqCst);
    }

    pub fn scores_descending<const T: usize>(&self) -> [TruncatedVec<(Id, u64)>; T] {
        const LIMIT: usize = 50;

        let slide = self.slide();

        std::array::from_fn(|i| {
            let current_slide = slide.checked_sub(i);

            match current_slide.and_then(|cs| self.slide_summaries.get(cs)) {
                None => TruncatedVec::default(),
                Some(s) => TruncatedVec::new(
                    s.scores_descending.iter().copied(),
                    LIMIT,
                    s.scores_descending.len(),
                ),
            }
        })
    }

    fn compute_final_summary(&self) -> FinalSummary {
        let summaries = self
            .slide_summaries
            .iter()
            .map(|(_, s)| s.points_earned.clone())
            .collect_vec();

        let summary_mapping: Vec<HashMap<_, _>> = summaries
            .iter()
            .map(|s| s.iter().copied().collect())
            .collect_vec();

        let scores_descending = self
            .slide_summaries
            .get(self.slide())
            .map_or(vec![], |s| s.scores_descending.clone());

        let id_to_points = |id| {
            summary_mapping
                .iter()
                .map(|h| *h.get(&id).unwrap_or(&0))
                .collect_vec()
        };

        FinalSummary {
            summary_descending: scores_descending
                .iter()
                .map(|(id, _)| (*id, id_to_points(*id)))
                .collect_vec(),
            mapping: scores_descending
                .into_iter()
                .map(|(id, _)| (id, id_to_points(id)))
                .collect(),
        }
    }

    fn final_summary(&self) -> &FinalSummary {
        self.final_summary
            .get_or_init(|| self.compute_final_summary())
    }

    pub fn host_summary(&self, limit: usize) -> TruncatedVec<(Id, Vec<u64>)> {
        let final_summary = self.final_summary();

        TruncatedVec::new(
            final_summary
                .summary_descending
                .iter()
                .map(|(id, points)| (*id, points.clone())),
            limit,
            final_summary.summary_descending.len(),
        )
    }

    pub fn player_summary(&self, id: Id) -> Vec<u64> {
        self.final_summary().mapping.get(&id).map_or(
            vec![0; self.slide_summaries.count()],
            std::clone::Clone::clone,
        )
    }

    pub fn score(&self, watcher_id: Id) -> Option<ScoreMessage> {
        let summary = self.slide_summaries.get(self.slide());
        summary.and_then(|s| {
            s.mapping
                .get(&watcher_id)
                .map(std::clone::Clone::clone)
                .map(|(points, position)| ScoreMessage { points, position })
        })
    }
}
