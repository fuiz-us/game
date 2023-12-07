use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
};

use actix_web::rt::time::Instant;
use actix_ws::Closed;
use erased_serde::serialize_trait_object;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::game_manager::watcher::WatcherValue;

use super::{
    fuiz::config::FuizConfig,
    leaderboard::{Leaderboard, LeaderboardMessage, ScoreMessage},
    names::{Names, NamesError},
    session::Tunnel,
    watcher::{WatcherError, WatcherId, WatcherValueKind, Watchers},
};

#[derive(Debug, Clone, Copy)]
pub enum GameState {
    WaitingScreen,
    Slide { index: usize, finished: bool },
    Done,
}

impl GameState {
    pub fn is_done(&self) -> bool {
        matches!(self, GameState::Done)
    }
}

pub struct Game<T: Tunnel> {
    pub fuiz_config: FuizConfig,
    pub watchers: Watchers<T>,
    pub names: Names,
    pub leaderboard: Leaderboard,
    pub state: Arc<Mutex<GameState>>,
    pub updated: Arc<Mutex<Instant>>,
}

impl<T: Tunnel> Debug for Game<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("fuiz", &self.fuiz_config)
            .finish()
    }
}

pub trait OutgoingMessage: Serialize + Clone {
    fn identifier(&self) -> &'static str;

    fn to_message(&self) -> Result<String, serde_json::Error> {
        Ok(format!(
            "{{\"{}\":{}}}",
            self.identifier(),
            serde_json::to_string(self)?
        ))
    }
}

pub trait StateMessage: erased_serde::Serialize {
    fn identifier(&self) -> &'static str;
}

serialize_trait_object!(StateMessage);

pub trait StateMessageSend {
    fn to_message(&self) -> Result<String, serde_json::Error>;
}

#[derive(Debug, Deserialize, Clone)]
pub enum IncomingMessage {
    Host(IncomingHostMessage),
    Unassigned(IncomingUnassignedMessage),
    Player(IncomingPlayerMessage),
}

impl IncomingMessage {
    fn follows(&self, sender_kind: &WatcherValueKind) -> bool {
        matches!(
            (self, sender_kind),
            (IncomingMessage::Host(_), WatcherValueKind::Host)
                | (IncomingMessage::Player(_), WatcherValueKind::Player)
                | (IncomingMessage::Unassigned(_), WatcherValueKind::Unassigned)
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

#[derive(Debug, Deserialize, Clone, Copy)]
pub enum IncomingHostMessage {
    Next,
    Index(usize),
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Clone)]
pub enum GameOutgoingMessage {
    WaitingScreen(WaitingScreenMessage),
    NameChoose,
    NameAssign(String),
    NameError(NamesError),
    Leaderboard { leaderboard: LeaderboardMessage },
    Score { score: Option<ScoreMessage> },
}

#[derive(Debug, Serialize, Clone)]
pub struct WaitingScreenMessage {
    exact_count: usize,
    players: Vec<String>,
    truncated: bool,
}

impl OutgoingMessage for GameOutgoingMessage {
    fn identifier(&self) -> &'static str {
        "Game"
    }
}

#[derive(Debug, Serialize, Clone)]
pub enum GameStateMessage {
    WaitingScreen(WaitingScreenMessage),
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
}

impl StateMessage for GameStateMessage {
    fn identifier(&self) -> &'static str {
        "Game"
    }
}

impl StateMessageSend for Box<dyn StateMessage> {
    fn to_message(&self) -> Result<String, serde_json::Error> {
        Ok(format!(
            "{{\"{}\":{}}}",
            self.identifier(),
            serde_json::to_string(self)?
        ))
    }
}

impl<T: Tunnel> Game<T> {
    pub fn new(fuiz: FuizConfig) -> Self {
        Self {
            fuiz_config: fuiz,
            watchers: Watchers::default(),
            names: Names::default(),
            leaderboard: Leaderboard::default(),
            state: Arc::new(Mutex::new(GameState::WaitingScreen)),
            updated: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub async fn play(&self) {
        self.change_state(GameState::Slide {
            index: 0,
            finished: false,
        });
        self.fuiz_config.play_slide(self, 0).await;
    }

    pub async fn finish_slide(&self) {
        if let GameState::Slide {
            index,
            finished: false,
        } = self.state()
        {
            self.change_state(GameState::Slide {
                index,
                finished: true,
            });

            self.announce_leaderboard().await;
        }
    }

    pub fn change_state(&self, game_state: GameState) {
        if let Ok(mut state) = self.state.lock() {
            *state = game_state;
        }
        self.update();
    }

    pub fn state(&self) -> GameState {
        if let Ok(state) = self.state.lock() {
            *state
        } else {
            GameState::Done
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
        let (exact_count, points) = self.leaderboard.get_scores_truncated();

        LeaderboardMessage {
            exact_count,
            points: points
                .into_iter()
                .map(|(i, s)| (self.names.get_name(&i).unwrap_or("Unknown".to_owned()), s))
                .collect_vec(),
        }
    }

    pub fn score(&self, watcher_id: WatcherId) -> Option<ScoreMessage> {
        self.leaderboard.score(watcher_id)
    }

    pub async fn announce_leaderboard(&self) {
        let leaderboard_message = self.leaderboard_message();

        self.announce_with(|watcher_id, watcher_kind| {
            Some(match watcher_kind {
                WatcherValueKind::Host | WatcherValueKind::Unassigned => {
                    GameOutgoingMessage::Leaderboard {
                        leaderboard: leaderboard_message.clone(),
                    }
                }
                WatcherValueKind::Player => GameOutgoingMessage::Score {
                    score: self.score(watcher_id),
                },
            })
        })
        .await;
    }

    pub async fn announce<O: OutgoingMessage>(&self, message: O) {
        let serialized_message = message
            .to_message()
            .expect("default enum serializer failed");

        let mut watchers_to_be_removed = Vec::new();

        for (watcher, session, _) in self.watchers.vec() {
            if session.send(&serialized_message).await.is_err() {
                watchers_to_be_removed.push(watcher);
            }
        }

        for watcher in watchers_to_be_removed {
            self.remove_watcher_session(watcher).await;
        }
    }

    pub fn get_name(&self, watcher_id: WatcherId) -> Option<String> {
        self.watchers
            .get_watcher_value(watcher_id)
            .and_then(|v| match v {
                WatcherValue::Player(x) => Some(x),
                _ => None,
            })
    }

    pub async fn announce_with<O, F>(&self, sender: F)
    where
        O: OutgoingMessage,
        F: Fn(WatcherId, WatcherValueKind) -> Option<O>,
    {
        let mut watchers_to_be_removed = Vec::new();

        for (watcher, session, v) in self.watchers.vec() {
            let Some(message) = sender(watcher, v.kind()) else {
                continue;
            };

            let Ok(serialized_message) = message.to_message() else {
                continue;
            };

            if session.send(&serialized_message).await.is_err() {
                watchers_to_be_removed.push(watcher);
            }
        }

        for watcher in watchers_to_be_removed {
            self.remove_watcher_session(watcher).await;
        }
    }

    pub async fn announce_host<O: OutgoingMessage>(&self, message: O) {
        let serialized_message = message
            .to_message()
            .expect("default enum serializer failed");

        let mut watchers_to_be_removed = Vec::new();

        for (watcher, session, _) in self.watchers.specific_vec(WatcherValueKind::Host) {
            if session.send(&serialized_message).await.is_err() {
                watchers_to_be_removed.push(watcher);
            }
        }

        for watcher in watchers_to_be_removed {
            self.remove_watcher_session(watcher).await;
        }
    }

    pub fn players(&self) -> Vec<WatcherId> {
        self.watchers
            .specific_vec(WatcherValueKind::Player)
            .into_iter()
            .map(|(x, _, _)| x)
            .collect_vec()
    }

    pub async fn send<O: OutgoingMessage>(&self, message: O, watcher_id: WatcherId) {
        let serialized_message = message
            .to_message()
            .expect("default enum serializer failed");

        self.watchers.send(&serialized_message, watcher_id).await;
    }

    pub async fn mark_as_done(&self) {
        self.change_state(GameState::Done);
        let watchers = self.watchers.vec().iter().map(|(x, _, _)| *x).collect_vec();
        for watcher in watchers {
            self.remove_watcher_session(watcher).await;
        }
    }

    pub async fn receive_message(&self, watcher_id: WatcherId, message: IncomingMessage) {
        let Some(watcher_value) = self.watchers.get_watcher_value(watcher_id) else {
            return;
        };

        if !message.follows(&watcher_value.kind()) {
            return;
        }

        self.update();

        match message {
            IncomingMessage::Unassigned(IncomingUnassignedMessage::NameRequest(s)) => {
                match self.names.set_name(watcher_id, s) {
                    Ok(resulting_name) => {
                        self.watchers.update_watcher_value(
                            watcher_id,
                            WatcherValue::Player(resulting_name.clone()),
                        );
                        self.send(GameOutgoingMessage::NameAssign(resulting_name), watcher_id)
                            .await;

                        self.announce_waiting().await;

                        self.send(
                            GameOutgoingMessage::WaitingScreen(self.get_waiting_message()),
                            watcher_id,
                        )
                        .await;
                    }
                    Err(e) => {
                        self.send(GameOutgoingMessage::NameError(e), watcher_id)
                            .await;
                    }
                }
            }
            message => match self.state() {
                GameState::WaitingScreen => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        self.play().await;
                    }
                }
                GameState::Slide { index, finished } => match finished {
                    false => {
                        self.fuiz_config
                            .receive_message(self, watcher_id, message, index)
                            .await
                    }
                    true => {
                        if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                            if index + 1 >= self.fuiz_config.len() {
                                self.change_state(GameState::Done)
                            } else {
                                self.change_state(GameState::Slide {
                                    index: index + 1,
                                    finished: false,
                                });
                                self.fuiz_config.play_slide(self, index + 1).await;
                            }
                        }
                    }
                },
                GameState::Done => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        self.mark_as_done().await;
                    }
                }
            },
        }
    }

    fn get_names(&self) -> Vec<String> {
        self.watchers
            .specific_vec(WatcherValueKind::Player)
            .into_iter()
            .filter_map(|(_, _, x)| match x {
                WatcherValue::Player(s) => Some(s.to_owned()),
                _ => None,
            })
            .collect_vec()
    }

    fn get_names_limited(&self, limit: usize) -> Vec<String> {
        self.watchers
            .specific_vec(WatcherValueKind::Player)
            .into_iter()
            .take(limit)
            .filter_map(|(_, _, x)| match x {
                WatcherValue::Player(s) => Some(s.to_owned()),
                _ => None,
            })
            .collect_vec()
    }

    pub fn state_message(
        &self,
        watcher_id: WatcherId,
        watcher_kind: WatcherValueKind,
    ) -> Box<dyn StateMessage> {
        match self.state() {
            GameState::WaitingScreen => {
                Box::new(GameStateMessage::WaitingScreen(self.get_waiting_message()))
            }
            GameState::Slide { index, finished } => match finished {
                false => self
                    .fuiz_config
                    .state_message(watcher_id, watcher_kind, self, index)
                    .expect("Index was violated"),
                true => Box::new(match watcher_kind {
                    WatcherValueKind::Host | WatcherValueKind::Unassigned => {
                        GameStateMessage::Leaderboard {
                            index,
                            count: self.fuiz_config.len(),
                            leaderboard: self.leaderboard_message(),
                        }
                    }
                    WatcherValueKind::Player => GameStateMessage::Score {
                        index,
                        count: self.fuiz_config.len(),
                        score: self.score(watcher_id),
                    },
                }),
            },
            GameState::Done => Box::new(match watcher_kind {
                WatcherValueKind::Host | WatcherValueKind::Unassigned => {
                    GameStateMessage::Leaderboard {
                        index: self.fuiz_config.len() - 1,
                        count: self.fuiz_config.len(),
                        leaderboard: self.leaderboard_message(),
                    }
                }
                WatcherValueKind::Player => GameStateMessage::Score {
                    index: self.fuiz_config.len() - 1,
                    count: self.fuiz_config.len(),
                    score: self.score(watcher_id),
                },
            }),
        }
    }

    pub fn get_waiting_message(&self) -> WaitingScreenMessage {
        let exact_count = self.watchers.specific_count(WatcherValueKind::Player);

        const LIMIT: usize = 50;

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

    pub async fn announce_waiting(&self) {
        if let GameState::WaitingScreen = self.state() {
            self.announce_host(GameOutgoingMessage::WaitingScreen(
                self.get_waiting_message(),
            ))
            .await;
        }
    }

    pub async fn add_unassigned(&self, watcher: WatcherId, session: T) -> Result<(), WatcherError> {
        self.watchers
            .add_watcher(watcher, WatcherValue::Unassigned, session)
            .await?;

        self.send(GameOutgoingMessage::NameChoose, watcher).await;

        Ok(())
    }

    pub fn reserve_watcher(
        &self,
        watcher: WatcherId,
        watcher_value: WatcherValue,
    ) -> Result<(), NamesError> {
        if let WatcherValue::Player(s) = watcher_value.clone() {
            self.names.set_name(watcher, s)?;
        }

        self.watchers.reserve_watcher(watcher, watcher_value);

        Ok(())
    }

    pub async fn update_session(&self, watcher_id: WatcherId, session: T) -> Result<(), Closed> {
        let Some(watcher_value) = self.watchers.get_watcher_value(watcher_id) else {
            return Ok(());
        };

        match watcher_value.clone() {
            WatcherValue::Host => {
                session
                    .send(
                        &self
                            .state_message(watcher_id, watcher_value.kind())
                            .to_message()
                            .expect("default serializer shouldn't fail"),
                    )
                    .await?;
            }
            WatcherValue::Player(name) => {
                session
                    .send(
                        &GameOutgoingMessage::NameAssign(name)
                            .to_message()
                            .expect("Serializer should never fail"),
                    )
                    .await?;

                session
                    .send(
                        &self
                            .state_message(watcher_id, watcher_value.kind())
                            .to_message()
                            .expect("default serializer shouldn't fail"),
                    )
                    .await?;
            }
            WatcherValue::Unassigned => {
                session
                    .send(
                        &GameOutgoingMessage::NameChoose
                            .to_message()
                            .expect("Serializer should never fail"),
                    )
                    .await?;
            }
        }

        self.watchers.update_watcher_session(watcher_id, session);

        self.announce_waiting().await;

        Ok(())
    }

    pub fn has_watcher(&self, watcher_id: WatcherId) -> bool {
        self.watchers.has_watcher(watcher_id)
    }

    pub async fn remove_watcher_session(&self, watcher: WatcherId) {
        self.watchers.remove_watcher_session(&watcher).await;
    }
}
