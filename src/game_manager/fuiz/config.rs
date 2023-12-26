use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::game_manager::{
    session::Tunnel,
    watcher::{Id, ValueKind},
    SyncMessage,
};

use super::{
    super::game::{Game, IncomingMessage},
    bingo,
    media::Media,
    multiple_choice,
};

const CONFIG: crate::config::fuiz::FuizConfig = crate::CONFIG.fuiz;

const MAX_SLIDES_COUNT: usize = CONFIG.max_slides_count.unsigned_abs() as usize;
const MAX_TITLE_LENGTH: usize = CONFIG.max_title_length.unsigned_abs() as usize;

const MAX_TEXT_LENGTH: usize = crate::CONFIG.fuiz.answer_text.max_length.unsigned_abs() as usize;

#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub enum TextOrMedia {
    Media(#[garde(skip)] Media),
    Text(#[garde(length(max = MAX_TEXT_LENGTH))] String),
}

#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub struct Fuiz {
    #[garde(length(max = MAX_TITLE_LENGTH))]
    title: String,
    #[garde(length(max = MAX_SLIDES_COUNT), dive)]
    slides: Vec<Slide>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub enum Slide {
    MultipleChoice(#[garde(dive)] multiple_choice::Slide),
    Bingo(#[garde(dive)] bingo::Slide),
}

impl Fuiz {
    pub fn len(&self) -> usize {
        self.slides.len()
    }

    pub async fn play_slide<T: Tunnel>(&self, game: &Game<T>, i: usize) {
        if let Some(slide) = self.slides.get(i) {
            slide.play(game, self, i, self.slides.len()).await;
        }
    }

    pub async fn receive_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        watcher_id: Id,
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
        watcher_id: Id,
        watcher_kind: ValueKind,
        game: &Game<T>,
        index: usize,
    ) -> Option<SyncMessage> {
        self.slides.get(index).map(|slide| {
            slide.state_message(watcher_id, watcher_kind, game, index, self.slides.len())
        })
    }
}

impl Slide {
    pub async fn play<T: Tunnel>(&self, game: &Game<T>, fuiz: &Fuiz, index: usize, count: usize) {
        match self {
            Self::MultipleChoice(s) => {
                s.play(game, fuiz, index, count).await;
            }
            Self::Bingo(s) => {
                s.play(game, fuiz, index, count);
            }
        }
    }

    pub async fn receive_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        fuiz: &Fuiz,
        watcher_id: Id,
        message: IncomingMessage,
        index: usize,
        count: usize,
    ) {
        match self {
            Self::MultipleChoice(s) => {
                s.receive_message(game, fuiz, watcher_id, message, index, count)
                    .await;
            }
            Self::Bingo(s) => {
                s.receive_message(game, fuiz, watcher_id, &message, index, count);
            }
        }
    }

    pub fn state_message<T: Tunnel>(
        &self,
        watcher_id: Id,
        watcher_kind: ValueKind,
        game: &Game<T>,
        index: usize,
        count: usize,
    ) -> SyncMessage {
        match self {
            Self::MultipleChoice(s) => SyncMessage::MultipleChoice(s.state_message(
                watcher_id,
                watcher_kind,
                game,
                index,
                count,
            )),
            Self::Bingo(s) => {
                SyncMessage::Bingo(s.state_message(watcher_id, watcher_kind, game, index, count))
            }
        }
    }
}
