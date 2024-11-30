use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{watcher::Id, TruncatedVec};

#[derive(Debug, Clone)]
pub struct FinalSummary {
    // for each slide, how many people earned points and how many didn't
    stats: Vec<(usize, usize)>,
    // for each player, the points they earned on each slide
    mapping: HashMap<Id, Vec<u64>>,
}

#[derive(Deserialize)]
struct LeaderboardSerde {
    points_earned: Vec<Vec<(Id, u64)>>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(from = "LeaderboardSerde")]
pub struct Leaderboard {
    points_earned: Vec<Vec<(Id, u64)>>,

    #[serde(skip)]
    previous_scores_descending: Vec<(Id, u64)>,
    #[serde(skip)]
    scores_descending: Vec<(Id, u64)>,
    #[serde(skip)]
    score_and_position: HashMap<Id, (u64, usize)>,
    #[serde(skip)]
    final_summary: once_cell_serde::sync::OnceCell<FinalSummary>,
}

impl From<LeaderboardSerde> for Leaderboard {
    fn from(serde: LeaderboardSerde) -> Self {
        let scores_descending = serde
            .points_earned
            .iter()
            .flat_map(|points_earned| points_earned.iter().copied())
            .sorted_by_key(|(_, points)| *points)
            .rev()
            .collect_vec();

        let previous_scores_descending = serde
            .points_earned
            .iter()
            .rev()
            .skip(1)
            .rev()
            .flat_map(|points_earned| points_earned.iter().copied())
            .sorted_by_key(|(_, points)| *points)
            .rev()
            .collect_vec();

        let score_and_position = scores_descending
            .iter()
            .enumerate()
            .map(|(i, (id, p))| (*id, (*p, i)))
            .collect();

        Leaderboard {
            points_earned: serde.points_earned,
            previous_scores_descending,
            scores_descending,
            score_and_position,
            final_summary: once_cell_serde::sync::OnceCell::new(),
        }
    }
}

#[derive(Debug, Serialize, Clone, Copy)]
pub struct ScoreMessage {
    pub points: u64,
    pub position: usize,
}

impl Leaderboard {
    pub fn add_scores(&mut self, scores: &[(Id, u64)]) {
        let mut summary: HashMap<Id, u64> = self
            .score_and_position
            .iter()
            .map(|(id, (points, _))| (*id, *points))
            .collect();

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

        self.points_earned.push(scores.to_vec());

        self.previous_scores_descending =
            std::mem::replace(&mut self.scores_descending, scores_descending);

        self.score_and_position = mapping;
    }

    pub fn last_two_scores_descending(&self) -> [TruncatedVec<(Id, u64)>; 2] {
        const LIMIT: usize = 50;

        [
            TruncatedVec::new(
                self.scores_descending.iter().copied(),
                LIMIT,
                self.scores_descending.len(),
            ),
            TruncatedVec::new(
                self.previous_scores_descending.iter().copied(),
                LIMIT,
                self.previous_scores_descending.len(),
            ),
        ]
    }

    fn compute_final_summary(&self, show_real_score: bool) -> FinalSummary {
        let map_score = |s: u64| {
            if show_real_score {
                s
            } else {
                s.min(1)
            }
        };

        FinalSummary {
            stats: self
                .points_earned
                .iter()
                .map(|points_earned| {
                    let earned_count = points_earned
                        .iter()
                        .filter(|(_, earned)| *earned > 0)
                        .count();

                    (earned_count, points_earned.len() - earned_count)
                })
                .collect(),
            mapping: self
                .points_earned
                .iter()
                .map(|points_earned| {
                    points_earned
                        .iter()
                        .map(|(id, points)| (*id, map_score(*points)))
                        .collect::<HashMap<_, _>>()
                })
                .enumerate()
                .fold(
                    HashMap::new(),
                    |mut aggregate_score_mapping, (slide_index, slide_score_mapping)| {
                        for (id, points) in slide_score_mapping {
                            aggregate_score_mapping.entry(id).or_default().push(points);
                        }
                        for (_, v) in aggregate_score_mapping.iter_mut() {
                            v.resize(slide_index + 1, 0);
                        }
                        aggregate_score_mapping
                    },
                ),
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
        self.final_summary(show_real_score)
            .mapping
            .get(&id)
            .map_or(vec![0; self.points_earned.len()], std::clone::Clone::clone)
    }

    pub fn score(&self, watcher_id: Id) -> Option<ScoreMessage> {
        let (points, position) = self.score_and_position.get(&watcher_id)?;
        Some(ScoreMessage {
            points: *points,
            position: *position,
        })
    }
}
