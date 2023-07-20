use serde::{Deserialize, Serialize};

use super::fuiz::Fuiz;

const GAME_ID_LENGTH: usize = 6;
const EASY_ALPHABET: [char; 20] = [
    'A', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'L', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y',
    'Z',
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GameId {
    id: String,
}

impl GameId {
    pub fn new() -> Self {
        Self {
            id: fastrand::choose_multiple(EASY_ALPHABET.into_iter(), GAME_ID_LENGTH)
                .into_iter()
                .collect(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Game {
    pub game_id: GameId,
    pub fuiz: Fuiz,
}
