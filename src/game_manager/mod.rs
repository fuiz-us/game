use std::sync::Arc;

use dashmap::{DashMap, mapref::entry::Entry};

use self::{
    fuiz::Fuiz,
    game::{Game, GameId},
};

pub mod fuiz;
pub mod game;
pub mod media;
pub mod theme;

#[derive(Debug, Default)]
pub struct GameManager {
    games: Arc<DashMap<GameId, Game>>,
}

impl GameManager {
    pub fn add_game(&self, fuiz: Fuiz) -> GameId {
        loop {
            let game_id = GameId::new();
            
            match self.games.entry(game_id.clone()) {
                Entry::Occupied(_) => continue,
                Entry::Vacant(v) => {
                    v.insert(Game { game_id: game_id.clone(), fuiz });
                    return game_id;
                }
            }
        }
    }
}
