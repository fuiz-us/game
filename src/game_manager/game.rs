use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
};

use actix_web::rt::time::Instant;
use actix_ws::Closed;
use erased_serde::serialize_trait_object;
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::game_manager::watcher::WatcherValue;

use super::{
    fuiz::config::FuizConfig,
    game_id::GameId,
    leaderboard::Leaderboard,
    names::{Names, NamesError},
    session::Tunnel,
    watcher::{WatcherId, WatcherValueKind, Watchers},
};

#[derive(Debug, Clone, Copy)]
pub enum GameState {
    WaitingScreen,
    Slide(usize),
    FinalLeaderboard,
}

impl GameState {
    pub fn is_done(&self) -> bool {
        matches!(self, GameState::FinalLeaderboard)
    }
}

pub struct Game<T: Tunnel> {
    pub game_id: GameId,
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
            .field("game_id", &self.game_id)
            .field("fuiz", &self.fuiz_config)
            .finish()
    }
}

pub trait OutcomingMessage: Serialize + Clone {
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
}

#[derive(Debug, Serialize, Clone)]
pub enum GameOutcomingMessage {
    WaitingScreen(WaitingScreenMessage),
    NameChoose,
    NameAssign(String),
    NameError(NamesError),
}

#[derive(Debug, Serialize, Clone)]
pub struct WaitingScreenMessage {
    exact_count: usize,
    players: Vec<String>,
    truncated: bool,
}

impl OutcomingMessage for GameOutcomingMessage {
    fn identifier(&self) -> &'static str {
        "Game"
    }
}

#[derive(Debug, Serialize, Clone)]
pub enum GameStateMessage {
    WaitingScreen(WaitingScreenMessage),
    Leaderboard(Vec<(String, u64)>),
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
    pub fn new(game_id: GameId, fuiz: FuizConfig) -> Self {
        Self {
            game_id,
            fuiz_config: fuiz,
            watchers: Watchers::default(),
            names: Names::default(),
            leaderboard: Leaderboard::default(),
            state: Arc::new(Mutex::new(GameState::WaitingScreen)),
            updated: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub async fn play(&self) {
        info!("PLAYING {}", self.game_id.id);
        self.fuiz_config.play(self).await;
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
            GameState::FinalLeaderboard
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

    pub fn leaderboard(&self) -> Vec<(String, u64)> {
        self.leaderboard
            .get_scores_descending()
            .into_iter()
            .map(|(i, s)| (self.names.get_name(&i).unwrap_or("Unknown".to_owned()), s))
            .collect_vec()
    }

    pub async fn announce<O: OutcomingMessage>(&self, message: O) {
        let serialized_message = message
            .to_message()
            .expect("default enum serializer failed");

        let mut watchers_to_be_removed = Vec::new();

        for (watcher, session, _) in self.watchers.iter() {
            if session.send(&serialized_message).await.is_err() {
                watchers_to_be_removed.push(watcher);
            }
        }

        for watcher in watchers_to_be_removed {
            self.remove_watcher_session(watcher);
        }
    }

    pub async fn announce_host<O: OutcomingMessage>(&self, message: O) {
        let serialized_message = message
            .to_message()
            .expect("default enum serializer failed");

        let mut watchers_to_be_removed = Vec::new();

        for (watcher, session, _) in self.watchers.specific_iter(WatcherValueKind::Host) {
            if session.send(&serialized_message).await.is_err() {
                watchers_to_be_removed.push(watcher);
            }
        }

        for watcher in watchers_to_be_removed {
            self.remove_watcher_session(watcher);
        }
    }

    pub async fn send<O: OutcomingMessage>(&self, message: O, watcher_id: WatcherId) {
        let serialized_message = message
            .to_message()
            .expect("default enum serializer failed");

        self.watchers.send(&serialized_message, watcher_id).await;
    }

    pub fn mark_as_done(&self) {
        self.change_state(GameState::FinalLeaderboard);
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
                        self.send(GameOutcomingMessage::NameAssign(resulting_name), watcher_id)
                            .await;

                        self.announce_waiting().await;

                        self.send(
                            GameOutcomingMessage::WaitingScreen(self.get_waiting_message()),
                            watcher_id,
                        )
                        .await;
                    }
                    Err(e) => {
                        self.send(GameOutcomingMessage::NameError(e), watcher_id)
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
                GameState::Slide(i) => {
                    self.fuiz_config
                        .receive_message(self, watcher_id, message, i)
                        .await;
                }
                GameState::FinalLeaderboard => {
                    if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                        self.mark_as_done();
                    }
                }
            },
        }
    }

    fn get_names(&self) -> Vec<String> {
        self.watchers
            .specific_iter(WatcherValueKind::Player)
            .into_iter()
            .filter_map(|(_, _, x)| match x {
                WatcherValue::Player(s) => Some(s.to_owned()),
                _ => None,
            })
            .collect_vec()
    }

    fn get_names_limited(&self, limit: usize) -> Vec<String> {
        self.watchers
            .specific_iter(WatcherValueKind::Player)
            .into_iter()
            .take(limit)
            .filter_map(|(_, _, x)| match x {
                WatcherValue::Player(s) => Some(s.to_owned()),
                _ => None,
            })
            .collect_vec()
    }

    pub fn state_message(&self) -> Box<dyn StateMessage> {
        match self.state() {
            GameState::WaitingScreen => {
                Box::new(GameStateMessage::WaitingScreen(self.get_waiting_message()))
            }
            GameState::Slide(i) => self
                .fuiz_config
                .state_message(self, i)
                .expect("Index was violated"),
            GameState::FinalLeaderboard => {
                Box::new(GameStateMessage::Leaderboard(self.leaderboard()))
            }
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
            self.announce_host(GameOutcomingMessage::WaitingScreen(
                self.get_waiting_message(),
            ))
            .await;
        }
    }

    pub async fn add_unassigned(&self, watcher: WatcherId, session: T) {
        self.watchers
            .add_watcher(watcher, WatcherValue::Unassigned, session);

        self.send(GameOutcomingMessage::NameChoose, watcher).await;
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
        match self.watchers.get_watcher_value(watcher_id) {
            Some(WatcherValue::Player(name)) => {
                session
                    .send(
                        &GameOutcomingMessage::NameAssign(name)
                            .to_message()
                            .expect("Serializer should never fail"),
                    )
                    .await?;
            }
            Some(WatcherValue::Unassigned) => {
                session
                    .send(
                        &GameOutcomingMessage::NameChoose
                            .to_message()
                            .expect("Serializer should never fail"),
                    )
                    .await?;
            }
            _ => {}
        }

        session
            .send(
                &self
                    .state_message()
                    .to_message()
                    .expect("default serializer shouldn't fail"),
            )
            .await?;

        self.watchers.update_watcher_session(watcher_id, session);

        self.announce_waiting().await;

        Ok(())
    }

    pub fn has_watcher(&self, watcher_id: WatcherId) -> bool {
        self.watchers.has_watcher(watcher_id)
    }

    pub fn remove_watcher_session(&self, watcher: WatcherId) {
        self.watchers.remove_watcher_session(&watcher);
    }
}

#[cfg(test)]
mod tests {
    use mockall::predicate;

    use crate::game_manager::{
        fuiz::config::FuizConfig,
        game::Game,
        game_id::GameId,
        session::MockTunnel,
        watcher::{WatcherId, WatcherValue},
    };

    #[actix_web::test]
    async fn waiting_screen() {
        let fuiz = FuizConfig::new("Title".to_owned(), vec![]);

        let game = Game::new(GameId::new(), fuiz);

        let mut mock_host = MockTunnel::new();

        let host_id = WatcherId::default();

        let mut mock_player = MockTunnel::new();

        let player_id = WatcherId::default();

        mock_host
            .expect_send()
            .with(predicate::eq(
                r#"{"Game":{"WaitingScreen":{"exact_count":0,"players":[],"truncated":false}}}"#,
            ))
            .returning(|_| Ok(()));

        mock_player
            .expect_send()
            .with(predicate::eq(r#"{"Game":"NameChoose"}"#))
            .times(1)
            .returning(|_| Ok(()));

        assert!(game.reserve_watcher(host_id, WatcherValue::Host).is_ok());
        assert!(game.update_session(host_id, mock_host).await.is_ok(),);

        game.add_unassigned(player_id, mock_player).await;
    }
}
