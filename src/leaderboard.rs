use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{watcher::Id, TruncatedVec};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlideSummary {
    scores_descending: Vec<(Id, u64)>,
    mapping: HashMap<Id, (u64, usize)>,
    points_earned: Vec<(Id, u64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalSummary {
    stats: Vec<(usize, usize)>,
    mapping: HashMap<Id, Vec<u64>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Leaderboard {
    slide_summaries: Vec<SlideSummary>,
    current_slide: usize,
    final_summary: once_cell_serde::sync::OnceCell<FinalSummary>,
}

#[derive(Debug, Serialize, Clone, Copy)]
pub struct ScoreMessage {
    pub points: u64,
    pub position: usize,
}

impl Leaderboard {
    fn slide(&self) -> usize {
        self.current_slide
    }

    pub fn add_scores(&mut self, scores: &[(Id, u64)]) {
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

        self.slide_summaries.push(SlideSummary {
            scores_descending,
            mapping,
            points_earned: scores.to_vec(),
        });

        self.current_slide = self.slide_summaries.len() - 1;
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

    fn compute_final_summary(&self, show_real_score: bool) -> FinalSummary {
        let map_score = |s: u64| {
            if show_real_score {
                s
            } else {
                s.min(1)
            }
        };

        let summaries = self
            .slide_summaries
            .iter()
            .map(|s| {
                s.points_earned
                    .clone()
                    .into_iter()
                    .map(|(i, s)| (i, map_score(s)))
                    .collect_vec()
            })
            .collect_vec();

        let summary_mapping: Vec<HashMap<_, _>> = summaries
            .iter()
            .map(|s| {
                s.iter()
                    .map(|(id, points)| (*id, map_score(*points)))
                    .collect()
            })
            .collect_vec();

        let scores_descending = self.slide_summaries.get(self.slide()).map_or(vec![], |s| {
            s.scores_descending
                .clone()
                .into_iter()
                .map(|(i, s)| (i, map_score(s)))
                .collect_vec()
        });

        let id_to_points = |id| {
            summary_mapping
                .iter()
                .map(|h| map_score(*h.get(&id).unwrap_or(&0)))
                .collect_vec()
        };

        FinalSummary {
            stats: self
                .slide_summaries
                .iter()
                .map(|s| {
                    s.points_earned
                        .iter()
                        .fold((0, 0), |(correct, wrong), (_, earned)| {
                            if *earned > 0 {
                                (correct + 1, wrong)
                            } else {
                                (correct, wrong + 1)
                            }
                        })
                })
                .collect_vec(),
            mapping: scores_descending
                .into_iter()
                .map(|(id, _)| (id, id_to_points(id)))
                .collect(),
        }
    }

    fn final_summary(&self, show_real_score: bool) -> &FinalSummary {
        self.final_summary
            .get_or_init(|| self.compute_final_summary(show_real_score))
    }

    pub fn host_summary(&self, show_real_score: bool) -> (usize, Vec<(usize, usize)>) {
        let final_summary = self.final_summary(show_real_score);

        (final_summary.mapping.len(), final_summary.stats.clone())
    }

    pub fn player_summary(&self, id: Id, show_real_score: bool) -> Vec<u64> {
        self.final_summary(show_real_score).mapping.get(&id).map_or(
            vec![0; self.slide_summaries.len()],
            std::clone::Clone::clone,
        )
    }

    pub fn score(&self, watcher_id: Id) -> Option<ScoreMessage> {
        let summary = self.slide_summaries.get(self.slide());
        summary.and_then(|s| {
            s.mapping
                .get(&watcher_id)
                .copied()
                .map(|(points, position)| ScoreMessage { points, position })
        })
    }
}
