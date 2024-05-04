use std::{collections::HashSet, fmt::Debug};

use garde::Validate;
use heck::ToTitleCase;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::game_manager::watcher::Value;

use super::{
    fuiz::{config::Fuiz, multiple_choice},
    leaderboard::{Leaderboard, ScoreMessage},
    names::{self, Names},
    session::Tunnel,
    teams::{self, TeamManager},
    watcher::{self, Id, PlayerValue, ValueKind, Watchers},
    AlarmMessage, TruncatedVec,
};

/// Game Phase
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum State {
    /// A waiting screen where current players are displayed
    WaitingScreen,
    /// (TEAM ONLY): A waiting screen where current teams are displayed
    TeamDisplay,
    Slide(usize),
    Leaderboard(usize),
    Done,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Validate)]
pub struct TeamOptions {
    /// maximum initial team size
    #[garde(range(min = 1, max = 5))]
    size: usize,
    /// whether to assign people to random teams or let them choose their preferences
    #[garde(skip)]
    assign_random: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Validate)]
pub struct Options {
    /// using random names for players (skips choosing names)
    #[garde(skip)]
    random_names: bool,
    /// whether to show answers on players devices or not
    #[garde(skip)]
    show_answers: bool,
    #[garde(skip)]
    no_leaderboard: bool,
    #[garde(dive)]
    teams: Option<TeamOptions>,
}

#[derive(Serialize, Deserialize)]
/// one game session
pub struct Game {
    original_fuiz_config: Fuiz,
    /// configuration to create the game
    fuiz_config: Fuiz,
    /// set of watchers listening to message actions
    pub watchers: Watchers,
    /// mapping of names used in the game
    names: Names,
    /// score mapping from players/teams to score
    pub leaderboard: Leaderboard,
    /// current phase of the game
    pub state: State,
    options: Options,
    /// indicates if a game is locked so new players aren't able to enter
    locked: bool,
    team_manager: Option<TeamManager>,
}

impl Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("fuiz", &self.fuiz_config)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Deserialize, Clone)]
pub enum IncomingMessage {
    Ghost(IncomingGhostMessage),
    Host(IncomingHostMessage),
    Unassigned(IncomingUnassignedMessage),
    Player(IncomingPlayerMessage),
}

impl IncomingMessage {
    fn follows(&self, sender_kind: ValueKind) -> bool {
        matches!(
            (self, sender_kind),
            (IncomingMessage::Host(_), ValueKind::Host)
                | (IncomingMessage::Player(_), ValueKind::Player)
                | (IncomingMessage::Unassigned(_), ValueKind::Unassigned)
        )
    }
}

#[derive(Debug, Deserialize, Clone)]
pub enum IncomingPlayerMessage {
    IndexAnswer(usize),
    ChooseTeammates(Vec<String>),
}

#[derive(Debug, Deserialize, Clone)]
pub enum IncomingUnassignedMessage {
    NameRequest(String),
}

#[derive(Debug, Deserialize, Clone)]
pub enum IncomingGhostMessage {
    DemandId,
    ClaimId(Id),
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub enum IncomingHostMessage {
    Next,
    Index(usize),
    Lock(bool),
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Clone)]
pub enum UpdateMessage {
    IdAssign(Id),
    WaitingScreen(TruncatedVec<String>),
    TeamDisplay(TruncatedVec<String>),
    NameChoose,
    NameAssign(String),
    NameError(names::Error),
    Leaderboard {
        leaderboard: LeaderboardMessage,
    },
    Score {
        score: Option<ScoreMessage>,
    },
    Summary(SummaryMessage),
    FindTeam(String),
    ChooseTeammates {
        max_selection: usize,
        available: Vec<(String, bool)>,
    },
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Clone)]
pub enum SyncMessage {
    WaitingScreen(TruncatedVec<String>),
    TeamDisplay(TruncatedVec<String>),
    Leaderboard {
        index: usize,
        count: usize,
        leaderboard: LeaderboardMessage,
    },
    Score {
        index: usize,
        count: usize,
        score: Option<ScoreMessage>,
    },
    Metainfo(MetainfoMessage),
    Summary(SummaryMessage),
    NotAllowed,
    FindTeam(String),
    ChooseTeammates {
        max_selection: usize,
        available: Vec<(String, bool)>,
    },
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Clone)]
pub enum SummaryMessage {
    Player {
        score: Option<ScoreMessage>,
        points: Vec<u64>,
        config: Fuiz,
    },
    Host {
        stats: Vec<(usize, usize)>,
        player_count: usize,
        config: Fuiz,
        options: Options,
    },
}

#[derive(Debug, Serialize, Clone)]
pub enum MetainfoMessage {
    Host { locked: bool },
    Player { score: u64, show_answers: bool },
}

#[derive(Debug, Serialize, Clone)]
pub struct LeaderboardMessage {
    pub current: TruncatedVec<(String, u64)>,
    pub prior: TruncatedVec<(String, u64)>,
}

// Convenience methods
impl Game {
    fn set_state(&mut self, game_state: State) {
        self.state = game_state;
    }

    fn score(&self, watcher_id: Id) -> Option<ScoreMessage> {
        self.leaderboard.score(self.leaderboard_id(watcher_id))
    }

    pub fn leaderboard_id(&self, player_id: Id) -> Id {
        match &self.team_manager {
            Some(team_manager) => team_manager.get_team(player_id).unwrap_or(player_id),
            None => player_id,
        }
    }

    fn choose_teammates_message<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &self,
        watcher: Id,
        team_manager: &TeamManager,
        tunnel_finder: F,
    ) -> UpdateMessage {
        let pref: HashSet<_> = team_manager
            .get_preferences(watcher)
            .unwrap_or_default()
            .into_iter()
            .collect();
        UpdateMessage::ChooseTeammates {
            max_selection: team_manager.optimal_size,
            available: self
                .watchers
                .specific_vec(ValueKind::Player, tunnel_finder)
                .into_iter()
                .filter_map(|(id, _, _)| Some((id, self.watchers.get_name(id)?)))
                .map(|(id, name)| (name, pref.contains(&id)))
                .collect(),
        }
    }

    fn waiting_screen_names<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &self,
        tunnel_finder: F,
    ) -> TruncatedVec<String> {
        const LIMIT: usize = 50;

        if let Some(team_manager) = &self.team_manager {
            if matches!(self.state, State::TeamDisplay) {
                return team_manager.team_names().unwrap_or_default();
            }
        }

        let player_names = self
            .watchers
            .specific_vec(ValueKind::Player, tunnel_finder)
            .into_iter()
            .filter_map(|(_, _, x)| match x {
                Value::Player(player_value) => Some(player_value.name().to_owned()),
                _ => None,
            })
            .unique();

        TruncatedVec::new(
            player_names,
            LIMIT,
            self.watchers.specific_count(ValueKind::Player),
        )
    }

    fn leaderboard_message(&self) -> LeaderboardMessage {
        let [current, prior] = self.leaderboard.scores_descending();

        let id_map = |i| self.names.get_name(&i).unwrap_or("Unknown".to_owned());

        let id_score_map = |(id, s)| (id_map(id), s);

        LeaderboardMessage {
            current: current.map(id_score_map),
            prior: prior.map(id_score_map),
        }
    }
}

impl Game {
    pub fn new(fuiz: Fuiz, options: Options, host_id: Id) -> Self {
        Self {
            original_fuiz_config: fuiz.clone(),
            fuiz_config: fuiz,
            watchers: Watchers::with_host_id(host_id),
            names: Names::default(),
            leaderboard: Leaderboard::default(),
            state: State::WaitingScreen,
            options,
            team_manager: options.teams.map(
                |TeamOptions {
                     size,
                     assign_random,
                 }| TeamManager::new(size, assign_random),
            ),
            locked: false,
        }
    }

    /// starts the game
    pub fn play<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        schedule_message: S,
        tunnel_finder: F,
    ) {
        if self.fuiz_config.len() > 0 {
            if let Some(team_manager) = &mut self.team_manager {
                if matches!(self.state, State::WaitingScreen) {
                    team_manager.finalize(&mut self.watchers, &mut self.names, &tunnel_finder);
                    self.state = State::TeamDisplay;
                    self.watchers.announce_with(
                        |id, kind| {
                            Some(match kind {
                                ValueKind::Player => UpdateMessage::FindTeam(
                                    team_manager
                                        .get_team(id)
                                        .and_then(|id| self.names.get_name(&id))
                                        .unwrap_or_default(),
                                )
                                .into(),
                                _ => UpdateMessage::TeamDisplay(
                                    team_manager.team_names().unwrap_or_default(),
                                )
                                .into(),
                            })
                        },
                        &tunnel_finder,
                    );
                    return;
                }
            }
            self.set_state(State::Slide(0));
            self.fuiz_config
                .play_slide(&self.watchers, schedule_message, tunnel_finder, 0);
        } else {
            self.announce_summary(tunnel_finder);
        }
    }

    /// mark the current slide as done
    pub fn finish_slide<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        schedule_message: S,
        tunnel_finder: F,
    ) {
        if let State::Slide(index) = self.state {
            if self.options.no_leaderboard {
                if index + 1 < self.fuiz_config.len() {
                    self.state = State::Slide(index + 1);
                    self.fuiz_config.play_slide(
                        &self.watchers,
                        schedule_message,
                        &tunnel_finder,
                        index + 1,
                    );
                } else {
                    self.announce_summary(tunnel_finder);
                }
            } else {
                self.set_state(State::Leaderboard(index));

                let leaderboard_message = self.leaderboard_message();

                self.watchers.announce_with(
                    |watcher_id, watcher_kind| {
                        Some(match watcher_kind {
                            ValueKind::Host | ValueKind::Unassigned => UpdateMessage::Leaderboard {
                                leaderboard: leaderboard_message.clone(),
                            }
                            .into(),
                            ValueKind::Player => UpdateMessage::Score {
                                score: self.score(watcher_id),
                            }
                            .into(),
                        })
                    },
                    tunnel_finder,
                );
            }
        }
    }

    /// sends summary (last slide) to everyone
    fn announce_summary<T: Tunnel, F: Fn(Id) -> Option<T>>(&mut self, tunnel_finder: F) {
        self.state = State::Done;

        self.watchers.announce_with(
            |id, vk| match vk {
                ValueKind::Host => Some(
                    UpdateMessage::Summary({
                        let (player_count, stats) =
                            self.leaderboard.host_summary(!self.options.no_leaderboard);

                        SummaryMessage::Host {
                            stats,
                            player_count,
                            config: self.original_fuiz_config.clone(),
                            options: self.options,
                        }
                    })
                    .into(),
                ),
                ValueKind::Player => Some(
                    UpdateMessage::Summary(SummaryMessage::Player {
                        score: if self.options.no_leaderboard {
                            None
                        } else {
                            self.score(id)
                        },
                        points: self
                            .leaderboard
                            .player_summary(self.leaderboard_id(id), !self.options.no_leaderboard),
                        config: self.original_fuiz_config.clone(),
                    })
                    .into(),
                ),
                ValueKind::Unassigned => None,
            },
            tunnel_finder,
        );
    }

    /// mark the game as done and disconnect players
    pub fn mark_as_done<T: Tunnel, F: Fn(Id) -> Option<T>>(&mut self, tunnel_finder: F) {
        self.state = State::Done;

        let watchers = self
            .watchers
            .vec(&tunnel_finder)
            .iter()
            .map(|(x, _, _)| *x)
            .collect_vec();

        for watcher in watchers {
            self.watchers
                .remove_watcher_session(&watcher, &tunnel_finder);
        }
    }

    /// send metainfo to player about the game
    fn update_player_with_options<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &self,
        watcher: Id,
        tunnel_finder: F,
    ) {
        self.watchers.send_state(
            &SyncMessage::Metainfo(MetainfoMessage::Player {
                score: self.score(watcher).map_or(0, |x| x.points),
                show_answers: self.options.show_answers,
            })
            .into(),
            watcher,
            tunnel_finder,
        );
    }

    /// start interactions with unassigned player
    fn handle_unassigned<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &mut self,
        watcher: Id,
        tunnel_finder: F,
    ) {
        if let Some(team_manager) = &mut self.team_manager {
            if let Some(name) = team_manager.add_player(watcher, &mut self.watchers) {
                self.update_player_with_name(watcher, &name, &tunnel_finder);
            }
        }

        if self.options.random_names {
            loop {
                let Some(name) = petname::petname(2, " ") else {
                    continue;
                };
                if self
                    .assign_player_name(watcher, &name.to_title_case(), &tunnel_finder)
                    .is_ok()
                {
                    break;
                }
            }
        } else {
            self.watchers
                .send_message(&UpdateMessage::NameChoose.into(), watcher, tunnel_finder);
        }
    }

    /// assigns a player a name
    fn assign_player_name<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &mut self,
        watcher: Id,
        name: &str,
        tunnel_finder: F,
    ) -> Result<(), names::Error> {
        let name = self.names.set_name(watcher, name)?;

        self.watchers.update_watcher_value(
            watcher,
            Value::Player(watcher::PlayerValue::Individual { name: name.clone() }),
        );

        self.update_player_with_name(watcher, &name, tunnel_finder);

        Ok(())
    }

    /// sends messages to the player about their new assigned name
    pub fn update_player_with_name<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &self,
        watcher: Id,
        name: &str,
        tunnel_finder: F,
    ) {
        self.watchers.send_message(
            &UpdateMessage::NameAssign(name.to_string()).into(),
            watcher,
            &tunnel_finder,
        );

        self.update_player_with_options(watcher, &tunnel_finder);

        if !name.is_empty() {
            // Announce to others of user joining
            if matches!(self.state, State::WaitingScreen) {
                if let Some(team_manager) = &self.team_manager {
                    if !team_manager.is_random_assignments() {
                        self.watchers.announce_with(
                            |id, value| match value {
                                ValueKind::Player => Some(
                                    self.choose_teammates_message(id, team_manager, &tunnel_finder)
                                        .into(),
                                ),
                                _ => None,
                            },
                            &tunnel_finder,
                        );
                    }
                }

                self.watchers.announce_specific(
                    ValueKind::Host,
                    &UpdateMessage::WaitingScreen(self.waiting_screen_names(&tunnel_finder)).into(),
                    &tunnel_finder,
                );
            }
        }

        self.watchers.send_state(
            &self.state_message(watcher, ValueKind::Player, &tunnel_finder),
            watcher,
            tunnel_finder,
        );
    }

    // Network

    /// add a new watcher with given id and session
    pub fn add_unassigned<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &mut self,
        watcher: Id,
        tunnel_finder: F,
    ) -> Result<(), watcher::Error> {
        self.watchers.add_watcher(watcher, Value::Unassigned)?;

        if !self.locked {
            self.handle_unassigned(watcher, tunnel_finder);
        }

        Ok(())
    }

    /// handle incoming message from watcher id
    pub fn receive_message<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        watcher_id: Id,
        message: IncomingMessage,
        mut schedule_message: S,
        tunnel_finder: F,
    ) {
        let Some(watcher_value) = self.watchers.get_watcher_value(watcher_id) else {
            return;
        };

        if !message.follows(watcher_value.kind()) {
            return;
        }

        match message {
            IncomingMessage::Unassigned(_) if self.locked => {}
            IncomingMessage::Host(IncomingHostMessage::Lock(lock_state)) => {
                self.locked = lock_state;
            }
            IncomingMessage::Unassigned(IncomingUnassignedMessage::NameRequest(s))
                if !self.options.random_names =>
            {
                if let Err(e) = self.assign_player_name(watcher_id, &s, &tunnel_finder) {
                    self.watchers.send_message(
                        &UpdateMessage::NameError(e).into(),
                        watcher_id,
                        tunnel_finder,
                    );
                }
            }
            IncomingMessage::Player(IncomingPlayerMessage::ChooseTeammates(preferences)) => {
                if let Some(team_manager) = &mut self.team_manager {
                    team_manager.set_preferences(
                        watcher_id,
                        preferences
                            .into_iter()
                            .filter_map(|name| self.names.get_id(&name))
                            .collect_vec(),
                    );
                }
            }
            message => match self.state {
                State::WaitingScreen | State::TeamDisplay => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        self.play(schedule_message, &tunnel_finder);
                    }
                }
                State::Slide(index) => {
                    if self.fuiz_config.receive_message(
                        &mut self.leaderboard,
                        &self.watchers,
                        self.team_manager.as_ref(),
                        &mut schedule_message,
                        &tunnel_finder,
                        watcher_id,
                        message,
                        index,
                    ) {
                        self.finish_slide(schedule_message, tunnel_finder);
                    }
                }
                State::Leaderboard(index) => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        if index + 1 >= self.fuiz_config.len() {
                            self.announce_summary(&tunnel_finder);
                        } else {
                            self.set_state(State::Slide(index + 1));
                            self.fuiz_config.play_slide(
                                &self.watchers,
                                schedule_message,
                                tunnel_finder,
                                index + 1,
                            );
                        }
                    }
                }
                State::Done => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        self.mark_as_done(tunnel_finder);
                    }
                }
            },
        }
    }

    pub fn receive_alarm<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        message: AlarmMessage,
        mut schedule_message: S,
        tunnel_finder: F,
    ) {
        match message {
            AlarmMessage::MultipleChoice(
                multiple_choice::AlarmMessage::ProceedFromSlideIntoSlide {
                    index: slide_index,
                    to: _,
                },
            ) => match self.state {
                State::Slide(current_index) if current_index == slide_index => {
                    if self.fuiz_config.receive_alarm(
                        &mut self.leaderboard,
                        &self.watchers,
                        (&self.team_manager).as_ref(),
                        &mut schedule_message,
                        &tunnel_finder,
                        message,
                        current_index,
                    ) {
                        self.finish_slide(schedule_message, tunnel_finder);
                    }
                }
                _ => (),
            },
        }
    }

    /// returns the message necessary to synchronize state
    pub fn state_message<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &self,
        watcher_id: Id,
        watcher_kind: ValueKind,
        tunnel_finder: F,
    ) -> super::SyncMessage {
        match self.state {
            State::WaitingScreen => match &self.team_manager {
                Some(team_manager)
                    if !team_manager.is_random_assignments()
                        && matches!(watcher_kind, ValueKind::Player) =>
                {
                    let pref: HashSet<Id> = team_manager
                        .get_preferences(watcher_id)
                        .unwrap_or_default()
                        .into_iter()
                        .collect();
                    SyncMessage::ChooseTeammates {
                        max_selection: team_manager.optimal_size,
                        available: self
                            .watchers
                            .specific_vec(ValueKind::Player, tunnel_finder)
                            .into_iter()
                            .filter_map(|(id, _, _)| Some((id, self.watchers.get_name(id)?)))
                            .map(|(id, name)| (name, pref.contains(&id)))
                            .collect(),
                    }
                    .into()
                }
                _ => SyncMessage::WaitingScreen(self.waiting_screen_names(tunnel_finder)).into(),
            },
            State::TeamDisplay => match watcher_kind {
                ValueKind::Player => SyncMessage::FindTeam(
                    self.team_manager
                        .as_ref()
                        .and_then(|tm| tm.get_team(watcher_id))
                        .and_then(|id| self.watchers.get_name(id))
                        .unwrap_or_default(),
                )
                .into(),
                _ => SyncMessage::TeamDisplay(
                    self.team_manager
                        .as_ref()
                        .and_then(teams::TeamManager::team_names)
                        .unwrap_or_default(),
                )
                .into(),
            },
            State::Leaderboard(index) => match watcher_kind {
                ValueKind::Host | ValueKind::Unassigned => SyncMessage::Leaderboard {
                    index,
                    count: self.fuiz_config.len(),
                    leaderboard: self.leaderboard_message(),
                }
                .into(),
                ValueKind::Player => SyncMessage::Score {
                    index,
                    count: self.fuiz_config.len(),
                    score: self.score(watcher_id),
                }
                .into(),
            },
            State::Slide(index) => self
                .fuiz_config
                .state_message(
                    watcher_id,
                    watcher_kind,
                    self.team_manager.as_ref(),
                    &self.watchers,
                    tunnel_finder,
                    index,
                )
                .expect("Index was violated"),
            State::Done => match watcher_kind {
                ValueKind::Host => SyncMessage::Summary({
                    let (player_count, stats) =
                        self.leaderboard.host_summary(!self.options.no_leaderboard);
                    SummaryMessage::Host {
                        stats,
                        player_count,
                        config: self.original_fuiz_config.clone(),
                        options: self.options,
                    }
                })
                .into(),
                ValueKind::Player => SyncMessage::Summary(SummaryMessage::Player {
                    score: if self.options.no_leaderboard {
                        None
                    } else {
                        self.score(watcher_id)
                    },
                    points: self.leaderboard.player_summary(
                        self.leaderboard_id(watcher_id),
                        !self.options.no_leaderboard,
                    ),
                    config: self.original_fuiz_config.clone(),
                })
                .into(),
                ValueKind::Unassigned => SyncMessage::NotAllowed.into(),
            },
        }
    }

    /// replaces the session associated with watcher id with a new one
    pub fn update_session<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &mut self,
        watcher_id: Id,
        tunnel_finder: F,
    ) {
        let Some(watcher_value) = self.watchers.get_watcher_value(watcher_id) else {
            return;
        };

        match watcher_value.clone() {
            Value::Host => {
                self.watchers.send_state(
                    &self.state_message(watcher_id, watcher_value.kind(), &tunnel_finder),
                    watcher_id,
                    &tunnel_finder,
                );
                self.watchers.send_state(
                    &SyncMessage::Metainfo(MetainfoMessage::Host {
                        locked: self.locked,
                    })
                    .into(),
                    watcher_id,
                    tunnel_finder,
                );
            }
            Value::Player(player_value) => {
                if let PlayerValue::Team {
                    team_name,
                    individual_name: _,
                    team_id: _,
                    player_index_in_team: _,
                } = &player_value
                {
                    self.watchers.send_message(
                        &UpdateMessage::FindTeam(team_name.clone()).into(),
                        watcher_id,
                        &tunnel_finder,
                    );
                }
                self.watchers.send_message(
                    &UpdateMessage::NameAssign(player_value.name().to_owned()).into(),
                    watcher_id,
                    &tunnel_finder,
                );
                self.update_player_with_options(watcher_id, &tunnel_finder);
                self.watchers.send_state(
                    &self.state_message(watcher_id, watcher_value.kind(), &tunnel_finder),
                    watcher_id,
                    &tunnel_finder,
                );
            }
            Value::Unassigned if self.locked => {}
            Value::Unassigned => {
                self.handle_unassigned(watcher_id, &tunnel_finder);
            }
        }
    }
}
