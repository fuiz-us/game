use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
};

use actix_web::rt::time::Instant;
use heck::ToTitleCase;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::game_manager::watcher::Value;

use super::{
    fuiz::config::Fuiz,
    leaderboard::{Leaderboard, Message, ScoreMessage},
    names::{self, Names},
    session::Tunnel,
    watcher::{self, Id, ValueKind, Watchers},
};

#[derive(Debug, Clone, Copy)]
pub enum State {
    WaitingScreen,
    Slide { index: usize, finished: bool },
    Done,
}

impl State {
    pub fn is_done(&self) -> bool {
        matches!(self, State::Done)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Options {
    random_names: bool,
}

pub struct Game<T: Tunnel> {
    fuiz_config: Fuiz,
    pub watchers: Watchers<T>,
    names: Names,
    pub leaderboard: Leaderboard,
    state: Arc<Mutex<State>>,
    updated: Arc<Mutex<Instant>>,
    options: Options,
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
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Clone)]
pub enum UpdateMessage {
    IdAssign(Id),
    WaitingScreen(WaitingScreenMessage),
    NameChoose,
    NameAssign(String),
    NameError(names::Error),
    Leaderboard { leaderboard: Message },
    Score { score: Option<ScoreMessage> },
}

#[derive(Debug, Serialize, Clone)]
pub struct WaitingScreenMessage {
    exact_count: usize,
    players: Vec<String>,
    truncated: bool,
}

#[derive(Debug, Serialize, Clone)]
pub enum SyncMessage {
    WaitingScreen(WaitingScreenMessage),
    Leaderboard {
        index: usize,
        count: usize,
        leaderboard: Message,
    },
    Score {
        index: usize,
        count: usize,
        score: Option<ScoreMessage>,
    },
}

impl<T: Tunnel> Game<T> {
    pub fn new(fuiz: Fuiz, options: Options) -> Self {
        Self {
            fuiz_config: fuiz,
            watchers: Watchers::default(),
            names: Names::default(),
            leaderboard: Leaderboard::default(),
            state: Arc::new(Mutex::new(State::WaitingScreen)),
            updated: Arc::new(Mutex::new(Instant::now())),
            options,
        }
    }

    pub async fn play(&self) {
        self.change_state(State::Slide {
            index: 0,
            finished: false,
        });
        self.fuiz_config.play_slide(self, 0).await;
    }

    pub fn finish_slide(&self) {
        if let State::Slide {
            index,
            finished: false,
        } = self.state()
        {
            self.change_state(State::Slide {
                index,
                finished: true,
            });

            self.announce_leaderboard();
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

    pub fn leaderboard_message(&self) -> Message {
        let (exact_count, points) = self.leaderboard.get_scores_truncated();

        Message {
            exact_count,
            points: points
                .into_iter()
                .map(|(i, s)| (self.names.get_name(&i).unwrap_or("Unknown".to_owned()), s))
                .collect_vec(),
        }
    }

    pub fn score(&self, watcher_id: Id) -> Option<ScoreMessage> {
        self.leaderboard.score(watcher_id)
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

    pub fn announce(&self, message: &super::UpdateMessage) {
        for (_, session, _) in self.watchers.vec() {
            session.send_message(message);
        }
    }

    pub fn get_name(&self, watcher_id: Id) -> Option<String> {
        self.watchers
            .get_watcher_value(watcher_id)
            .and_then(|v| match v {
                Value::Player(x) => Some(x),
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
        self.watchers
            .specific_vec(ValueKind::Player)
            .into_iter()
            .map(|(x, _, _)| x)
            .collect_vec()
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

    pub async fn receive_message(&self, watcher_id: Id, message: IncomingMessage) {
        let Some(watcher_value) = self.watchers.get_watcher_value(watcher_id) else {
            return;
        };

        if !message.follows(watcher_value.kind()) {
            return;
        }

        self.update();

        match message {
            IncomingMessage::Unassigned(IncomingUnassignedMessage::NameRequest(s))
                if !self.options.random_names =>
            {
                if let Err(e) = self.assign_name(watcher_id, s) {
                    self.send(&UpdateMessage::NameError(e).into(), watcher_id);
                }
            }
            message => match self.state() {
                State::WaitingScreen => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        self.play().await;
                    }
                }
                State::Slide { index, finished } => {
                    if finished {
                        if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                            if index + 1 >= self.fuiz_config.len() {
                                self.change_state(State::Done);
                            } else {
                                self.change_state(State::Slide {
                                    index: index + 1,
                                    finished: false,
                                });
                                self.fuiz_config.play_slide(self, index + 1).await;
                            }
                        }
                    } else {
                        self.fuiz_config
                            .receive_message(self, watcher_id, message, index)
                            .await;
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

    fn get_names(&self) -> Vec<String> {
        self.watchers
            .specific_vec(ValueKind::Player)
            .into_iter()
            .filter_map(|(_, _, x)| match x {
                Value::Player(s) => Some(s),
                _ => None,
            })
            .collect_vec()
    }

    fn get_names_limited(&self, limit: usize) -> Vec<String> {
        self.watchers
            .specific_vec(ValueKind::Player)
            .into_iter()
            .take(limit)
            .filter_map(|(_, _, x)| match x {
                Value::Player(s) => Some(s),
                _ => None,
            })
            .collect_vec()
    }

    pub fn state_message(&self, watcher_id: Id, watcher_kind: ValueKind) -> super::SyncMessage {
        match self.state() {
            State::WaitingScreen => SyncMessage::WaitingScreen(self.get_waiting_message()).into(),
            State::Slide { index, finished } => {
                if finished {
                    match watcher_kind {
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
                    }
                } else {
                    self.fuiz_config
                        .state_message(watcher_id, watcher_kind, self, index)
                        .expect("Index was violated")
                }
            }
            State::Done => match watcher_kind {
                ValueKind::Host | ValueKind::Unassigned => SyncMessage::Leaderboard {
                    index: self.fuiz_config.len() - 1,
                    count: self.fuiz_config.len(),
                    leaderboard: self.leaderboard_message(),
                }
                .into(),
                ValueKind::Player => SyncMessage::Score {
                    index: self.fuiz_config.len() - 1,
                    count: self.fuiz_config.len(),
                    score: self.score(watcher_id),
                }
                .into(),
            },
        }
    }

    pub fn get_waiting_message(&self) -> WaitingScreenMessage {
        const LIMIT: usize = 50;

        let exact_count = self.watchers.specific_count(ValueKind::Player);

        if exact_count < LIMIT {
            let names = self.get_names();
            WaitingScreenMessage {
                exact_count: names.len(),
                players: names,
                truncated: false,
            }
        } else {
            let names = self.get_names_limited(LIMIT);
            WaitingScreenMessage {
                exact_count,
                players: names,
                truncated: true,
            }
        }
    }

    pub fn announce_waiting(&self) {
        if let State::WaitingScreen = self.state() {
            self.announce_host(&UpdateMessage::WaitingScreen(self.get_waiting_message()).into());
        }
    }

    pub fn add_unassigned(&self, watcher: Id, session: T) -> Result<(), watcher::Error> {
        self.watchers
            .add_watcher(watcher, Value::Unassigned, session)?;

        self.handle_unassigned(watcher);

        Ok(())
    }

    fn assign_name(&self, watcher: Id, name: String) -> Result<(), names::Error> {
        let name = self.names.set_name(watcher, &name)?;

        self.watchers
            .update_watcher_value(watcher, Value::Player(name.clone()));

        self.send(&UpdateMessage::NameAssign(name).into(), watcher);

        self.announce_waiting();

        self.send_state(&self.state_message(watcher, ValueKind::Player), watcher);

        Ok(())
    }

    fn handle_unassigned(&self, watcher: Id) {
        if self.options.random_names {
            loop {
                let name = petname::petname(2, " ").to_title_case();
                if self.assign_name(watcher, name).is_ok() {
                    break;
                }
            }
        } else {
            self.send(&UpdateMessage::NameChoose.into(), watcher);
        }
    }

    pub fn reserve_host(&self, watcher: Id) {
        self.watchers.reserve_watcher(watcher, Value::Host);
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
            }
            Value::Player(name) => {
                self.send(&UpdateMessage::NameAssign(name).into(), watcher_id);
                self.send_state(
                    &self.state_message(watcher_id, watcher_value.kind()),
                    watcher_id,
                );
            }
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
