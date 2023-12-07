use std::sync::Arc;

use concread::CowCell;
use derive_where::derive_where;
use enum_map::EnumMap;

use self::{fuiz::config::FuizConfig, game::Game, game_id::GameId, session::Tunnel};

pub mod fuiz;
pub mod game;
pub mod game_id;
pub mod leaderboard;
pub mod names;
pub mod session;
pub mod watcher;

#[derive_where(Debug)]
struct SharedGame<T: Tunnel>(CowCell<Option<Arc<Game<T>>>>);

impl<T: Tunnel> Default for SharedGame<T> {
    fn default() -> Self {
        Self(CowCell::new(None))
    }
}

#[derive_where(Debug, Default)]
pub struct GameManager<T: Tunnel> {
    games: EnumMap<GameId, SharedGame<T>>,
}

impl<T: Tunnel> GameManager<T> {
    pub fn add_game(&self, fuiz: FuizConfig) -> GameId {
        let shared_game = Arc::new(Game::new(fuiz));

        loop {
            let game_id = GameId::new();

            let Some(mut game) = self.games[game_id].0.try_write() else {
                continue;
            };

            if game.is_none() {
                *game = Some(shared_game);
                game.commit();
                return game_id;
            }
        }
    }

    pub fn get_game(&self, game_id: &GameId) -> Option<Arc<Game<T>>> {
        (*self.games[*game_id].0.read()).clone()
    }

    pub fn remove_game(&self, game_id: &GameId) {
        let mut game = self.games[*game_id].0.write();
        *game = None;
        game.commit();
    }
}
