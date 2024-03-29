use std::{
    fmt::Debug,
    sync::{atomic::AtomicBool, Mutex},
};

use actix_web::rt::time::Instant;
use garde::Validate;
use heck::ToTitleCase;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::game_manager::watcher::Value;

use super::{
    fuiz::config::Fuiz,
    leaderboard::{Leaderboard, ScoreMessage},
    names::{self, Names},
    session::Tunnel,
    teams::{self, TeamManager},
    watcher::{self, Id, ValueKind, Watchers},
    TruncatedVec,
};

#[derive(Debug, Clone, Copy)]
pub enum State {
    WaitingScreen,
    TeamDisplay,
    Slide(usize),
    Leaderboard(usize),
    Done,
}

impl State {
    pub fn is_done(&self) -> bool {
        matches!(self, State::Done)
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, Validate)]
pub struct Options {
    #[garde(skip)]
    random_names: bool,
    #[garde(skip)]
    show_answers: bool,
    #[garde(skip)]
    no_leaderboard: bool,
    #[garde(range(min = 1, max = 5))]
    teams: Option<usize>,
}

pub struct Game<T: Tunnel> {
    fuiz_config: Fuiz,
    pub watchers: Watchers<T>,
    names: Names,
    pub leaderboard: Leaderboard,
    state: Mutex<State>,
    updated: Mutex<Instant>,
    options: Options,
    locked: AtomicBool,
    team_manager: Option<TeamManager>,
}

impl<T: Tunnel> Debug for Game<T> {
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
    Leaderboard { leaderboard: LeaderboardMessage },
    Score { score: Option<ScoreMessage> },
    Summary(SummaryMessage),
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

impl<T: Tunnel> Game<T> {
    pub fn new(fuiz: Fuiz, options: Options, host_id: Id) -> Self {
        Self {
            fuiz_config: fuiz,
            watchers: Watchers::with_host_id(host_id),
            names: Names::default(),
            leaderboard: Leaderboard::default(),
            state: Mutex::new(State::WaitingScreen),
            updated: Mutex::new(Instant::now()),
            options,
            team_manager: options.teams.map(TeamManager::new),
            locked: AtomicBool::default(),
        }
    }

    pub async fn play(&self) {
        if self.fuiz_config.len() > 0 {
            if let Some(team_manager) = &self.team_manager {
                if matches!(self.state(), State::WaitingScreen) {
                    team_manager.finalize(self, &self.watchers, &self.names);
                    self.change_state(State::TeamDisplay);
                    self.announce_host(
                        &UpdateMessage::TeamDisplay(team_manager.team_names().unwrap_or_default())
                            .into(),
                    );
                    return;
                }
            }
            self.change_state(State::Slide(0));
            self.fuiz_config.play_slide(self, 0).await;
        } else {
            self.announce_summary();
        }
    }

    pub async fn finish_slide(&self) {
        if let State::Slide(index) = self.state() {
            if self.options.no_leaderboard {
                if index + 1 < self.fuiz_config.len() {
                    self.change_state(State::Slide(index + 1));
                    self.fuiz_config.play_slide(self, index + 1).await;
                } else {
                    self.announce_summary();
                }
            } else {
                self.change_state(State::Leaderboard(index));

                self.announce_leaderboard();
            }
        }
    }

    pub fn change_state(&self, game_state: State) {
        if let Ok(mut state) = self.state.lock() {
            *state = game_state;
        }
        self.update();
    }

    pub fn state(&self) -> State {
        if let Ok(state) = self.state.lock() {
            *state
        } else {
            State::Done
        }
    }

    pub fn update(&self) {
        if let Ok(mut updated) = self.updated.lock() {
            *updated = Instant::now();
        }
    }

    pub fn updated(&self) -> Instant {
        if let Ok(updated) = self.updated.lock() {
            *updated
        } else {
            Instant::now()
        }
    }

    pub fn leaderboard_message(&self) -> LeaderboardMessage {
        let [current, prior] = self.leaderboard.scores_descending();

        let id_map = |i| self.names.get_name(&i).unwrap_or("Unknown".to_owned());

        let id_score_map = |(id, s)| (id_map(id), s);

        LeaderboardMessage {
            current: current.map(id_score_map),
            prior: prior.map(id_score_map),
        }
    }

    pub fn score(&self, watcher_id: Id) -> Option<ScoreMessage> {
        self.leaderboard.score(self.leaderboard_id(watcher_id))
    }

    pub fn announce_leaderboard(&self) {
        let leaderboard_message = self.leaderboard_message();

        self.announce_with(|watcher_id, watcher_kind| {
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
        });
    }

    fn announce_summary(&self) {
        self.change_state(State::Done);

        self.announce_with(|id, vk| match vk {
            ValueKind::Host => Some(
                UpdateMessage::Summary({
                    let (player_count, stats) =
                        self.leaderboard.host_summary(!self.options.no_leaderboard);

                    SummaryMessage::Host {
                        stats,
                        player_count,
                        config: self.fuiz_config.clone(),
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
                    config: self.fuiz_config.clone(),
                })
                .into(),
            ),
            ValueKind::Unassigned => None,
        });
    }

    pub fn announce(&self, message: &super::UpdateMessage) {
        for (_, session, _) in self.watchers.vec() {
            session.send_message(message);
        }
    }

    pub fn get_name(&self, watcher_id: Id) -> Option<String> {
        self.watchers
            .get_watcher_value(watcher_id)
            .and_then(|v| match v {
                Value::Player(player_value) => Some(player_value.name().to_owned()),
                _ => None,
            })
    }

    pub fn announce_with<F>(&self, sender: F)
    where
        F: Fn(Id, ValueKind) -> Option<super::UpdateMessage>,
    {
        for (watcher, session, v) in self.watchers.vec() {
            let Some(message) = sender(watcher, v.kind()) else {
                continue;
            };

            session.send_message(&message);
        }
    }

    pub fn announce_host(&self, message: &super::UpdateMessage) {
        for (_, session, _) in self.watchers.specific_vec(ValueKind::Host) {
            session.send_message(message);
        }
    }

    pub fn players(&self) -> Vec<Id> {
        match &self.team_manager {
            Some(team_manager) => team_manager.all_ids(),
            None => self
                .watchers
                .specific_vec(ValueKind::Player)
                .into_iter()
                .map(|(x, _, _)| x)
                .collect_vec(),
        }
    }

    pub fn send(&self, message: &super::UpdateMessage, watcher_id: Id) {
        self.watchers.send_message(message, watcher_id);
    }

    pub fn send_state(&self, message: &super::SyncMessage, watcher_id: Id) {
        self.watchers.send_state(message, watcher_id);
    }

    pub fn mark_as_done(&self) {
        self.change_state(State::Done);
        let watchers = self.watchers.vec().iter().map(|(x, _, _)| *x).collect_vec();
        for watcher in watchers {
            self.remove_watcher_session(watcher);
        }
    }

    pub fn leaderboard_id(&self, player_id: Id) -> Id {
        match &self.team_manager {
            Some(team_manager) => team_manager.get_team(player_id).unwrap_or(player_id),
            None => player_id,
        }
    }

    pub fn is_team(&self) -> bool {
        self.options.teams.is_some()
    }

    pub fn team_size(&self, player_id: Id) -> usize {
        match &self.team_manager {
            Some(team_manager) => team_manager.team_size(player_id).unwrap_or(1),
            None => 1,
        }
    }

    pub fn team_index(&self, player_id: Id) -> usize {
        match &self.team_manager {
            Some(team_manager) => team_manager.team_index(player_id).unwrap_or(0),
            None => 0,
        }
    }

    pub fn locked(&self) -> bool {
        self.locked.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub async fn receive_message(&self, watcher_id: Id, message: IncomingMessage) {
        let Some(watcher_value) = self.watchers.get_watcher_value(watcher_id) else {
            return;
        };

        if !message.follows(watcher_value.kind()) {
            return;
        }

        self.update();

        let locked = self.locked();

        match message {
            IncomingMessage::Unassigned(_) if locked => {}
            IncomingMessage::Host(IncomingHostMessage::Lock(lock_state)) => {
                self.locked
                    .store(lock_state, std::sync::atomic::Ordering::SeqCst);
            }
            IncomingMessage::Unassigned(IncomingUnassignedMessage::NameRequest(s))
                if !self.options.random_names =>
            {
                if let Err(e) = self.assign_name(watcher_id, &s) {
                    self.send(&UpdateMessage::NameError(e).into(), watcher_id);
                }
            }
            message => match self.state() {
                State::WaitingScreen | State::TeamDisplay => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        self.play().await;
                    }
                }
                State::Slide(index) => {
                    self.fuiz_config
                        .receive_message(self, watcher_id, message, index)
                        .await;
                }
                State::Leaderboard(index) => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        if index + 1 >= self.fuiz_config.len() {
                            self.announce_summary();
                        } else {
                            self.change_state(State::Slide(index + 1));
                            self.fuiz_config.play_slide(self, index + 1).await;
                        }
                    }
                }
                State::Done => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        self.mark_as_done();
                    }
                }
            },
        }
    }

    fn get_names(&self) -> impl Iterator<Item = String> {
        self.watchers
            .specific_vec(ValueKind::Player)
            .into_iter()
            .filter_map(|(_, _, x)| match x {
                Value::Player(player_value) => Some(player_value.name().to_owned()),
                _ => None,
            })
            .unique()
    }

    pub fn state_message(&self, watcher_id: Id, watcher_kind: ValueKind) -> super::SyncMessage {
        match self.state() {
            State::WaitingScreen => SyncMessage::WaitingScreen(self.get_waiting_message()).into(),
            State::TeamDisplay => SyncMessage::TeamDisplay(
                self.team_manager
                    .as_ref()
                    .and_then(teams::TeamManager::team_names)
                    .unwrap_or_default(),
            )
            .into(),
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
                .state_message(watcher_id, watcher_kind, self, index)
                .expect("Index was violated"),
            State::Done => match watcher_kind {
                ValueKind::Host => SyncMessage::Summary({
                    let (player_count, stats) =
                        self.leaderboard.host_summary(!self.options.no_leaderboard);
                    SummaryMessage::Host {
                        stats,
                        player_count,
                        config: self.fuiz_config.clone(),
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
                    config: self.fuiz_config.clone(),
                })
                .into(),
                ValueKind::Unassigned => SyncMessage::NotAllowed.into(),
            },
        }
    }

    pub fn get_waiting_message(&self) -> TruncatedVec<String> {
        const LIMIT: usize = 50;

        let exact_count = self.watchers.specific_count(ValueKind::Player);

        TruncatedVec::new(self.get_names(), LIMIT, exact_count)
    }

    pub fn announce_waiting(&self) {
        if let State::WaitingScreen = self.state() {
            self.announce_host(&UpdateMessage::WaitingScreen(self.get_waiting_message()).into());
        }
    }

    pub fn add_unassigned(&self, watcher: Id, session: T) -> Result<(), watcher::Error> {
        self.watchers
            .add_watcher(watcher, Value::Unassigned, session)?;

        if !self.locked() {
            self.handle_unassigned(watcher);
        }

        Ok(())
    }

    fn update_player_with_options(&self, watcher: Id) {
        self.send_state(
            &SyncMessage::Metainfo(MetainfoMessage::Player {
                score: self.score(watcher).map_or(0, |x| x.points),
                show_answers: self.options.show_answers,
            })
            .into(),
            watcher,
        );
    }

    fn assign_name(&self, watcher: Id, name: &str) -> Result<(), names::Error> {
        let name = self.names.set_name(watcher, name)?;

        self.watchers.update_watcher_value(
            watcher,
            Value::Player(watcher::PlayerValue::Individual { name: name.clone() }),
        );

        self.update_user_with_name(watcher, &name);

        Ok(())
    }

    pub fn update_user_with_name(&self, watcher: Id, name: &str) {
        self.send(&UpdateMessage::NameAssign(name.to_string()).into(), watcher);

        self.update_player_with_options(watcher);

        if !name.is_empty() {
            self.announce_waiting();
        }

        self.send_state(&self.state_message(watcher, ValueKind::Player), watcher);
    }

    fn handle_unassigned(&self, watcher: Id) {
        if let Some(team_manager) = &self.team_manager {
            team_manager.add_player(watcher, self, &self.watchers);
        } else if self.options.random_names {
            loop {
                let name = petname::petname(2, " ").to_title_case();
                if self.assign_name(watcher, &name).is_ok() {
                    break;
                }
            }
        } else {
            self.send(&UpdateMessage::NameChoose.into(), watcher);
        }
    }

    pub fn update_session(&self, watcher_id: Id, session: T) {
        let Some(watcher_value) = self.watchers.get_watcher_value(watcher_id) else {
            return;
        };

        self.watchers.update_watcher_session(watcher_id, session);

        match watcher_value.clone() {
            Value::Host => {
                self.send_state(
                    &self.state_message(watcher_id, watcher_value.kind()),
                    watcher_id,
                );
                self.send_state(
                    &SyncMessage::Metainfo(MetainfoMessage::Host {
                        locked: self.locked(),
                    })
                    .into(),
                    watcher_id,
                );
            }
            Value::Player(player_value) => {
                self.send(
                    &UpdateMessage::NameAssign(player_value.name().to_owned()).into(),
                    watcher_id,
                );
                self.update_player_with_options(watcher_id);
                self.send_state(
                    &self.state_message(watcher_id, watcher_value.kind()),
                    watcher_id,
                );
            }
            Value::Unassigned if self.locked() => {}
            Value::Unassigned => {
                self.handle_unassigned(watcher_id);
            }
        }
    }

    pub fn has_watcher(&self, watcher_id: Id) -> bool {
        self.watchers.has_watcher(watcher_id)
    }

    pub fn remove_watcher_session(&self, watcher: Id) {
        self.watchers.remove_watcher_session(&watcher);
    }
}
