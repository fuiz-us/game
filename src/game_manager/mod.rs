use std::sync::Arc;

use dashmap::{mapref::entry::Entry, DashMap};
use derive_where::derive_where;

use self::{fuiz::config::FuizConfig, game::Game, game_id::GameId, session::Tunnel};

pub mod fuiz;
pub mod game;
pub mod game_id;
pub mod leaderboard;
pub mod names;
pub mod session;
pub mod watcher;

#[derive_where(Debug, Default)]
pub struct GameManager<T: Tunnel> {
    games: DashMap<GameId, Arc<Game<T>>>,
}

impl<T: Tunnel> GameManager<T> {
    pub fn add_game(&self, fuiz: FuizConfig) -> GameId {
        loop {
            let game_id = GameId::new();

            match self.games.entry(game_id.clone()) {
                Entry::Occupied(_) => continue,
                Entry::Vacant(v) => {
                    let game = Arc::new(Game::new(game_id.clone(), fuiz));
                    v.insert(game);
                    return game_id;
                }
            }
        }
    }

    pub fn get_game(&self, game_id: &GameId) -> Option<Arc<Game<T>>> {
        self.games.get(game_id).map(|g| g.value().to_owned())
    }

    pub fn remove_game(&self, game_id: &GameId) {
        self.games.remove(game_id);
    }
}
