use std::{
    collections::BTreeSet,
    sync::{atomic::AtomicUsize, OnceLock},
};

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
    player_to_team: ClashMap<Id, Id>,
    team_to_players: ClashMap<Id, boxcar::Vec<Id>>,
    pub optimal_size: usize,
    preferences: Option<ClashMap<Id, Vec<Id>>>,
    teams: OnceLock<Vec<(Id, String)>>,
    next_team_to_receive_player: AtomicUsize,
}

impl TeamManager {
    pub fn new(optimal_size: usize, assign_random: bool) -> Self {
        Self {
            player_to_team: ClashMap::default(),
            team_to_players: ClashMap::default(),
            optimal_size,
            preferences: if assign_random {
                None
            } else {
                Some(ClashMap::default())
            },
            teams: OnceLock::new(),
            next_team_to_receive_player: AtomicUsize::new(0),
        }
    }

    pub fn is_random_assignments(&self) -> bool {
        self.preferences.is_none()
    }

    pub fn finalize<T: Tunnel>(
        &self,
        _game: &Game<T>,
        watchers: &Watchers<T>,
        names: &names::Names,
    ) {
        self.teams.get_or_init(|| {
            let players = watchers
                .specific_vec(watcher::ValueKind::Player)
                .into_iter()
                .map(|(id, _, _)| id)
                .collect_vec();

            let teams_count = players.len().div_ceil(self.optimal_size).max(1);

            dbg!(players
                .iter()
                .map(|p| self.get_preferences(*p))
                .collect_vec());

            let mut existing_teams = players
                .into_iter()
                .map(|id| {
                    (
                        self.get_preferences(id)
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|pref| {
                                self.get_preferences(*pref)
                                    .unwrap_or_default()
                                    .into_iter()
                                    .any(|prefs_pref| prefs_pref == id)
                            })
                            .min()
                            .unwrap_or(id)
                            .min(id),
                        id,
                    )
                })
                .sorted()
                .group_by(|(smallest_moot, _)| *smallest_moot)
                .into_iter()
                .map(|(_, g)| {
                    // to guard against attacks
                    let mut players = g.map(|(_, player_id)| player_id).collect_vec();
                    fastrand::shuffle(&mut players);
                    players
                })
                .sorted_by_key(std::vec::Vec::len)
                .rev()
                .collect_vec();

            if existing_teams.len() > teams_count {
                #[derive(PartialEq, Eq, PartialOrd, Ord)]
                struct PreferenceGroup(usize, Vec<Id>);

                impl From<Vec<Id>> for PreferenceGroup {
                    fn from(value: Vec<Id>) -> Self {
                        Self(value.len(), value)
                    }
                }

                let mut tree: BTreeSet<PreferenceGroup> = BTreeSet::new();

                for prefs in existing_teams {
                    if let Some(bucket) = tree
                        .range(..(PreferenceGroup(self.optimal_size - prefs.len() + 1, Vec::new())))
                        .next_back()
                        .map(|b| b.1.clone())
                    {
                        tree.remove(&bucket.clone().into());
                        tree.insert(prefs.into_iter().chain(bucket).collect_vec().into());
                    } else {
                        tree.insert(prefs.into());
                    }
                }

                existing_teams = tree.into_iter().map(|p| p.1).collect_vec();
            }

            let final_teams = existing_teams
                .into_iter()
                .map(|players| {
                    let team_id = Id::new();

                    let team_name = loop {
                        match names.set_name(
                            team_id,
                            &pluralizer::pluralize(
                                &petname::petname(1, " ").to_title_case(),
                                2,
                                false,
                            ),
                        ) {
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
                                    individual_name: names.get_name(&player_id).unwrap_or_default(),
                                    team_id,
                                    player_index_in_team,
                                }),
                            );
                        },
                    );
                    self.team_to_players
                        .insert(team_id, players.iter().copied().collect());

                    (team_id, team_name)
                })
                .collect_vec();

            final_teams
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

    pub fn get_preferences(&self, player_id: Id) -> Option<Vec<Id>> {
        self.preferences.as_ref().and_then(|p| p.get(&player_id))
    }

    pub fn set_preferences(&self, player_id: Id, preferences: Vec<Id>) {
        if let Some(prefs) = &self.preferences {
            prefs.insert(player_id, preferences);
        }
    }

    pub fn add_player<T: Tunnel>(&self, player_id: Id, game: &Game<T>, watchers: &Watchers<T>) {
        if let Some(teams) = self.teams.get() {
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
                    individual_name: game.get_name(player_id).unwrap_or_default(),
                    team_id: *team_id,
                    player_index_in_team: player_index,
                }),
            );

            game.update_user_with_name(player_id, team_name);
        }
    }

    pub fn _team_size(&self, player_id: Id) -> Option<usize> {
        self.get_team(player_id)
            .and_then(|team_id| self.team_to_players.get(&team_id))
            .map(|p| p.count())
    }

    pub fn team_members(&self, player_id: Id) -> Option<Vec<Id>> {
        self.get_team(player_id).and_then(|team_id| {
            self.team_to_players
                .get(&team_id)
                .map(|v| v.iter().map(|(_, id)| *id).collect_vec())
        })
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
