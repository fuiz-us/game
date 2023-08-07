use std::{
    fmt::Debug,
    sync::{Arc, Mutex},
};

use actix_web::rt::time::Instant;
use erased_serde::serialize_trait_object;
use itertools::Itertools;
use serde::{Deserialize, Serialize};

use crate::game_manager::watcher::WatcherType;

use super::{
    fuiz::config::FuizConfig,
    game_id::GameId,
    leaderboard::Leaderboard,
    names::{Names, NamesError},
    session::Tunnel,
    watcher::{Watcher, Watchers},
    GameManager,
};

#[derive(Debug, Clone, Copy)]
pub enum GameState {
    WaitingScreen,
    Slide(usize),
    FinalLeaderboard,
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

pub trait OutcomingMessage: Serialize + Clone {}

pub trait StateMessage: erased_serde::Serialize {}

serialize_trait_object!(StateMessage);

#[derive(Debug, Deserialize, Clone, Copy)]
pub enum IncomingMessage {
    Host(IncomingHostMessage),
    Player(IncomingPlayerMessage),
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub enum IncomingPlayerMessage {
    IndexAnswer(usize),
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub enum IncomingHostMessage {
    Next,
}

#[derive(Debug, Serialize, Clone)]
pub enum GameOutcomingMessage {
    WaitingScreen(Vec<String>),
}

impl OutcomingMessage for GameOutcomingMessage {}

#[derive(Debug, Serialize, Clone)]
pub enum GameStateMessage {
    WaitingScreen(Vec<String>),
    Leaderboard(Vec<(String, u64)>),
}

impl StateMessage for GameStateMessage {}

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
        let serialized_message =
            serde_json::to_string(&message).expect("default enum serializer failed");

        for watcher in self.watchers.iter() {
            let session = watcher.value();
            if session.send(&serialized_message).await.is_err() {
                self.remove_watcher(watcher.key().to_owned());
            }
        }
    }

    pub async fn receive_message(
        &self,
        game_manager: &GameManager<T>,
        watcher: Watcher,
        message: IncomingMessage,
    ) {
        info!("GOT {:?} FROM {:?}", message, watcher);
        if !matches!(
            (watcher.kind, message),
            (WatcherType::Host, IncomingMessage::Host(_))
                | (WatcherType::Player(_), IncomingMessage::Player(_))
        ) {
            return;
        }

        let state = match self.state.lock() {
            Ok(state) => *state,
            _ => return,
        };

        match state {
            GameState::WaitingScreen => {
                if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                    self.play().await;
                }
            }
            GameState::Slide(i) => {
                self.fuiz_config
                    .receive_message(self, watcher.id, message, i)
                    .await;
            }
            GameState::FinalLeaderboard => {
                if let IncomingMessage::Host(IncomingHostMessage::Next) = message {
                    game_manager.remove_game(&self.game_id);
                }
            }
        }
    }

    pub fn state_message(&self) -> Box<dyn StateMessage> {
        match self.state() {
            GameState::WaitingScreen => Box::new(GameStateMessage::WaitingScreen(
                self.watchers
                    .players_iter()
                    .filter_map(|w| match w.key().kind.to_owned() {
                        WatcherType::Player(s) => Some(s),
                        WatcherType::Host => None,
                    })
                    .collect_vec(),
            )),
            GameState::Slide(i) => self
                .fuiz_config
                .state_message(self, i)
                .expect("Index was violated"),
            GameState::FinalLeaderboard => {
                Box::new(GameStateMessage::Leaderboard(self.leaderboard()))
            }
        }
    }

    fn players(&self) -> Vec<String> {
        self.watchers
            .players_iter()
            .filter_map(|w| match w.key().kind.to_owned() {
                WatcherType::Player(s) => Some(s),
                WatcherType::Host => None,
            })
            .collect_vec()
    }

    pub async fn add_watcher(&self, watcher: Watcher, session: T) -> Result<(), NamesError> {
        info!("HI {:?}", watcher);

        if let WatcherType::Player(s) = watcher.kind.clone() {
            self.names.set_name(watcher.id, s)?;
        }

        self.watchers.add_watcher(watcher, session);

        if matches!(self.state(), GameState::WaitingScreen) {
            self.announce(GameOutcomingMessage::WaitingScreen(self.players()))
                .await;
        }

        Ok(())
    }

    pub fn remove_watcher(&self, watcher: Watcher) {
        info!("BYE BYE {:?}", &watcher);
        self.watchers.remove_watcher(&watcher);
    }
}

#[cfg(test)]
mod tests {
    use mockall::{predicate, Sequence};
    use uuid::Uuid;

    use crate::game_manager::{
        fuiz::{
            config::FuizConfig,
            media::{Image, InternetImage},
            theme::Theme,
        },
        game::Game,
        game_id::GameId,
        session::MockTunnel,
        watcher::{Watcher, WatcherType},
    };

    #[actix_web::test]
    async fn waiting_screen() {
        let fuiz = FuizConfig::new(
            "Title".to_owned(),
            "Description".to_owned(),
            Image::Internet(InternetImage {
                url: "https://gitlab.com/adhami3310/Impression/-/avatar".to_owned(),
                alt: "impression avatar".to_owned(),
            }),
            Theme::Classic,
            vec![],
        );

        let game = Game::new(GameId::new(), fuiz);

        assert!(game.players().is_empty());

        let mut mock_host = MockTunnel::new();

        let host = Watcher {
            id: Uuid::new_v4(),
            kind: WatcherType::Host,
        };

        let mut mock_player = MockTunnel::new();

        let player = Watcher {
            id: Uuid::new_v4(),
            kind: WatcherType::Player("Barish".to_owned()),
        };

        let mut sequence_host = Sequence::new();

        mock_host
            .expect_send()
            .with(predicate::eq(r#"{"WaitingScreen":[]}"#))
            .times(1)
            .in_sequence(&mut sequence_host)
            .returning(|_| Ok(()));

        mock_host
            .expect_send()
            .with(predicate::eq(r#"{"WaitingScreen":["Barish"]}"#))
            .times(1)
            .in_sequence(&mut sequence_host)
            .returning(|_| Ok(()));

        mock_player
            .expect_send()
            .with(predicate::eq(r#"{"WaitingScreen":["Barish"]}"#))
            .times(1)
            .returning(|_| Ok(()));

        assert_eq!(game.add_watcher(host, mock_host).await, Ok(()));
        assert_eq!(game.add_watcher(player, mock_player).await, Ok(()));
    }
}
