use serde::{Deserialize, Serialize};

use crate::game_manager::{game::StateMessage, session::Tunnel, watcher::WatcherId};

use super::{
    super::game::{Game, GameState, IncomingMessage},
    multiple_choice,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FuizConfig {
    title: String,
    slides: Vec<Slide>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Slide {
    MultipleChoice(multiple_choice::Slide),
}

impl FuizConfig {
    #[cfg(test)]
    pub fn new(title: String, slides: Vec<Slide>) -> Self {
        Self { title, slides }
    }

    pub async fn play<T: Tunnel>(&self, game: &Game<T>) {
        self.play_slide(game, 0).await;
    }

    pub async fn play_slide<T: Tunnel>(&self, game: &Game<T>, i: usize) {
        if let Some(slide) = self.slides.get(i) {
            game.change_state(GameState::Slide(i));
            slide.play(game, self, i, self.slides.len()).await;
        } else {
            game.change_state(GameState::FinalLeaderboard);
        }
    }

    pub async fn receive_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        watcher_id: WatcherId,
        message: IncomingMessage,
        index: usize,
    ) {
        if let Some(slide) = self.slides.get(index) {
            slide
                .receive_message(game, self, watcher_id, message, index, self.slides.len())
                .await;
        }
    }

    pub fn state_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        index: usize,
    ) -> Option<Box<dyn StateMessage>> {
        if let Some(slide) = self.slides.get(index) {
            Some(slide.state_message(game, index, self.slides.len()))
        } else {
            None
        }
    }
}

impl Slide {
    pub async fn play<T: Tunnel>(
        &self,
        game: &Game<T>,
        fuiz: &FuizConfig,
        index: usize,
        count: usize,
    ) {
        match self {
            Self::MultipleChoice(s) => {
                s.play(game, fuiz, index, count).await;
            }
        }
    }

    pub async fn receive_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        fuiz: &FuizConfig,
        watcher_id: WatcherId,
        message: IncomingMessage,
        index: usize,
        count: usize,
    ) {
        match self {
            Self::MultipleChoice(s) => {
                s.receive_message(game, fuiz, watcher_id, message, index, count)
                    .await;
            }
        }
    }

    pub fn state_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        index: usize,
        count: usize,
    ) -> Box<dyn StateMessage> {
        match self {
            Self::MultipleChoice(s) => Box::new(s.state_message(game, index, count)),
        }
    }
}
