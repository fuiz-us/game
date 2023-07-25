use std::sync::Arc;

use dashmap::{mapref::entry::Entry, DashMap};

use self::{
    fuiz::Fuiz,
    game::{Game, GameId},
};

pub mod fuiz;
pub mod game;
pub mod media;
pub mod session;
pub mod theme;

#[derive(Debug, Default)]
pub struct GameManager {
    games: DashMap<GameId, Arc<Game>>,
}

impl GameManager {
    pub fn add_game(&self, fuiz: Fuiz) -> GameId {
        loop {
            let game_id = GameId::new();

            match self.games.entry(game_id.clone()) {
                Entry::Occupied(_) => continue,
                Entry::Vacant(v) => {
                    let game = Arc::new(Game {
                        game_id: game_id.clone(),
                        listeners: DashMap::new(),
                        fuiz,
                    });
                    v.insert(game);
                    return game_id;
                }
            }
        }
    }

    pub fn get_game(&self, game_id: &GameId) -> Option<Arc<Game>> {
        self.games.get(game_id).map(|g| g.value().to_owned())
    }
}
