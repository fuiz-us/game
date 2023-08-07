use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::game_manager::{game::StateMessage, session::Tunnel};

use super::{
    super::game::{Game, GameState, IncomingMessage},
    media::Image,
    multiple_choice,
    theme::Theme,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FuizConfig {
    title: String,
    description: String,
    thumbnail: Image,
    theme: Theme,
    slides: Vec<Slide>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Slide {
    MultipleChoice(multiple_choice::Slide),
}

impl FuizConfig {
    #[cfg(test)]
    pub fn new(
        title: String,
        description: String,
        thumbnail: Image,
        theme: Theme,
        slides: Vec<Slide>,
    ) -> Self {
        Self {
            title,
            description,
            thumbnail,
            theme,
            slides,
        }
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
        uuid: Uuid,
        message: IncomingMessage,
        index: usize,
    ) {
        if let Some(slide) = self.slides.get(index) {
            slide
                .receive_message(game, self, uuid, message, index)
                .await;
        }
    }

    pub fn state_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        index: usize,
    ) -> Option<Box<dyn StateMessage>> {
        if let Some(slide) = self.slides.get(index) {
            Some(slide.state_message(game))
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
        slides_count: usize,
    ) {
        match self {
            Self::MultipleChoice(s) => {
                s.play(game, fuiz, index, slides_count).await;
            }
        }
    }

    pub async fn receive_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        fuiz: &FuizConfig,
        uuid: Uuid,
        message: IncomingMessage,
        index: usize,
    ) {
        match self {
            Self::MultipleChoice(s) => {
                s.receive_message(game, fuiz, uuid, message, index).await;
            }
        }
    }

    pub fn state_message<T: Tunnel>(&self, game: &Game<T>) -> Box<dyn StateMessage> {
        match self {
            Self::MultipleChoice(s) => Box::new(s.state_message(game)),
        }
    }
}
