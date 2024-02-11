use std::{
    sync::{atomic::AtomicUsize, OnceLock},
    vec,
};

use dashmap::DashMap;
use heck::ToTitleCase;
use itertools::Itertools;

use crate::clashmap::ClashMap;

use super::{
    game::Game,
    names,
    session::Tunnel,
    watcher::{self, Id, Watchers},
    TruncatedVec,
};

#[derive(Debug)]
pub struct TeamManager {
    pending_players: boxcar::Vec<Id>,
    player_to_team: ClashMap<Id, Id>,
    team_to_players: DashMap<Id, boxcar::Vec<Id>>,
    optimal_size: usize,
    teams: OnceLock<Vec<(Id, String)>>,
    next_team_to_receive_player: AtomicUsize,
}

impl TeamManager {
    pub fn new(optimal_size: usize) -> Self {
        Self {
            player_to_team: ClashMap::default(),
            team_to_players: DashMap::default(),
            optimal_size,
            teams: OnceLock::new(),
            next_team_to_receive_player: AtomicUsize::new(0),
            pending_players: boxcar::Vec::new(),
        }
    }

    pub fn finalize<T: Tunnel>(
        &self,
        game: &Game<T>,
        watchers: &Watchers<T>,
        names: &names::Names,
    ) {
        self.teams.get_or_init(|| {
            let mut players = self
                .pending_players
                .iter()
                .map(|(_, v)| *v)
                .unique()
                .collect_vec();
            fastrand::shuffle(&mut players);
            let teams_count = players.len().div_ceil(self.optimal_size).max(1);
            let small_size = num_integer::div_floor(players.len(), teams_count);
            let large_size = players.len().div_ceil(teams_count);
            let large_size_count = players.len() - (small_size * teams_count);

            let (large_part, small_part) = players.split_at(large_size * large_size_count);

            self.next_team_to_receive_player
                .store(large_size_count, std::sync::atomic::Ordering::SeqCst);

            let large_part_chunks = if large_part.is_empty() {
                Vec::new()
            } else {
                large_part.chunks_exact(large_size).collect_vec()
            };

            let small_part_chunks = if small_part.is_empty() {
                Vec::new()
            } else {
                small_part.chunks_exact(small_size).collect_vec()
            };

            let additional_team_if_no_team: Vec<&[Id]> =
                if large_part_chunks.is_empty() && small_part_chunks.is_empty() {
                    vec![&[]]
                } else {
                    Vec::new()
                };

            let teams = large_part_chunks
                .into_iter()
                .chain(small_part_chunks)
                .chain(additional_team_if_no_team)
                .map(|players| {
                    let team_id = Id::new();

                    let team_name = loop {
                        match names.set_name(team_id, &petname::petname(2, " ").to_title_case()) {
                            Ok(unique_name) => break unique_name,
                            Err(_) => continue,
                        };
                    };

                    players.iter().copied().enumerate().for_each(
                        |(player_index_in_team, player_id)| {
                            self.player_to_team.insert(player_id, team_id);
                            watchers.update_watcher_value(
                                player_id,
                                watcher::Value::Player(watcher::PlayerValue::Team {
                                    team_name: team_name.clone(),
                                    team_id,
                                    player_index_in_team,
                                }),
                            );

                            game.update_user_with_name(player_id, &team_name);
                        },
                    );
                    self.team_to_players
                        .insert(team_id, players.iter().copied().collect());

                    (team_id, team_name)
                })
                .collect_vec();

            teams
        });
    }

    pub fn team_names(&self) -> Option<TruncatedVec<String>> {
        self.teams.get().map(|v| {
            TruncatedVec::new(
                v.iter().map(|(_, team_name)| team_name.to_owned()),
                50,
                v.len(),
            )
        })
    }

    pub fn get_team(&self, player_id: Id) -> Option<Id> {
        self.player_to_team.get(&player_id)
    }

    pub fn add_player<T: Tunnel>(&self, player_id: Id, game: &Game<T>, watchers: &Watchers<T>) {
        match self.teams.get() {
            None => {
                self.pending_players.push(player_id);
                game.update_user_with_name(player_id, "");
            }
            Some(teams) => {
                let next_index = self
                    .next_team_to_receive_player
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

                let (team_id, team_name) = teams
                    .get(next_index % teams.len())
                    .expect("there is always at least one team");

                self.player_to_team.insert(player_id, *team_id);
                let p = self
                    .team_to_players
                    .get(team_id)
                    .expect("race condition :(");

                let player_index = {
                    match p.iter().position(|(_, p)| *p == player_id) {
                        Some(i) => i,
                        None => p.push(player_id),
                    }
                };

                watchers.update_watcher_value(
                    player_id,
                    watcher::Value::Player(watcher::PlayerValue::Team {
                        team_name: team_name.to_owned(),
                        team_id: *team_id,
                        player_index_in_team: player_index,
                    }),
                );

                game.update_user_with_name(player_id, team_name);
            }
        }
    }

    pub fn team_size(&self, player_id: Id) -> Option<usize> {
        self.get_team(player_id)
            .and_then(|team_id| self.team_to_players.get(&team_id))
            .map(|p| p.count())
    }

    pub fn team_index(&self, player_id: Id) -> Option<usize> {
        self.get_team(player_id)
            .and_then(|team_id| self.team_to_players.get(&team_id))
            .and_then(|p| {
                p.iter().find_map(|(index, current_player_id)| {
                    if *current_player_id == player_id {
                        Some(index)
                    } else {
                        None
                    }
                })
            })
    }

    pub fn all_ids(&self) -> Vec<Id> {
        self.teams.get().map_or(Vec::new(), |teams| {
            teams.iter().map(|(id, _)| *id).collect_vec()
        })
    }
}
