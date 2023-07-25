use std::fmt::Debug;

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{fuiz::Fuiz, session::Session};

const GAME_ID_LENGTH: usize = 6;
const EASY_ALPHABET: [char; 20] = [
    'A', 'C', 'D', 'E', 'F', 'G', 'H', 'J', 'K', 'L', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y',
    'Z',
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GameId {
    pub id: String,
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

pub struct Game {
    pub game_id: GameId,
    pub fuiz: Fuiz,
    pub listeners: DashMap<Uuid, Session>,
}

impl Debug for Game {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Game")
            .field("game_id", &self.game_id)
            .field("fuiz", &self.fuiz)
            .finish()
    }
}

#[derive(Debug, Serialize, Clone, Copy)]
enum OutcomingMessage {
    PeopleCount(u64),
}

#[derive(Debug, Deserialize, Clone, Copy)]
pub enum IncomingMessage {
    WhenAreWeStarting,
}

impl Game {
    pub async fn start(&self) {
        loop {
            self.announce(OutcomingMessage::PeopleCount(self.listeners.len() as u64))
                .await;
            actix_web::rt::time::sleep(std::time::Duration::from_secs(5)).await;
        }
    }

    async fn announce(&self, message: OutcomingMessage) {
        let serialized_message =
            serde_json::to_string(&message).expect("default enum serializer failed");

        for listener in self.listeners.iter() {
            let session = listener.value();
            if session.send(&serialized_message).await.is_err() {
                self.remove_listener(listener.key().to_owned());
            }
        }
    }

    pub async fn receive_message(&self, id: Uuid, message: IncomingMessage) {
        info!("GOT {:?} FROM {}", message, id);
    }

    pub fn add_listener(&self, id: Uuid, session: Session) {
        info!("HI {}", id);
        self.listeners.insert(id, session);
    }

    pub fn remove_listener(&self, id: Uuid) {
        info!("BYE BYE {}", &id);
        self.listeners.remove(&id);
    }
}
