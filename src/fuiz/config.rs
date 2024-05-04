use web_time;

use garde::Validate;
use serde::{Deserialize, Serialize};

use crate::{
    leaderboard::Leaderboard, session::Tunnel, teams::TeamManager, watcher::{Id, ValueKind, Watchers}, AlarmMessage, SyncMessage
};

use super::{super::game::IncomingMessage, media::Media, multiple_choice};

const CONFIG: crate::config::fuiz::FuizConfig = crate::CONFIG.fuiz;

const MAX_SLIDES_COUNT: usize = CONFIG.max_slides_count.unsigned_abs() as usize;
const MAX_TITLE_LENGTH: usize = CONFIG.max_title_length.unsigned_abs() as usize;

const MAX_TEXT_LENGTH: usize = crate::CONFIG.fuiz.answer_text.max_length.unsigned_abs() as usize;

#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub enum TextOrMedia {
    Media(#[garde(skip)] Media),
    Text(#[garde(length(max = MAX_TEXT_LENGTH))] String),
}

/// A fuiz configuration, title is unused
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
}

impl Fuiz {
    pub fn len(&self) -> usize {
        self.slides.len()
    }

    pub fn play_slide<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        watchers: &Watchers,
        schedule_message: S,
        tunnel_finder: F,
        index: usize,
    ) {
        let count = self.len();
        if let Some(slide) = self.slides.get_mut(index) {
            slide.play(watchers, schedule_message, tunnel_finder, index, count);
        }
    }

    pub fn receive_message<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        leaderboard: &mut Leaderboard,
        watchers: &Watchers,
        team_manager: Option<&TeamManager>,
        schedule_message: S,
        tunnel_finder: F,
        watcher_id: Id,
        message: IncomingMessage,
        index: usize,
    ) -> bool {
        let count = self.len();

        if let Some(slide) = self.slides.get_mut(index) {
            slide.receive_message(
                leaderboard,
                watchers,
                team_manager,
                schedule_message,
                watcher_id,
                tunnel_finder,
                message,
                index,
                count,
            )
        } else {
            false
        }
    }

    pub fn state_message<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &self,
        watcher_id: Id,
        watcher_kind: ValueKind,
        team_manager: Option<&TeamManager>,
        watchers: &Watchers,
        tunnel_finder: F,
        index: usize,
    ) -> Option<SyncMessage> {
        self.slides.get(index).map(|slide| {
            slide.state_message(
                watcher_id,
                watcher_kind,
                team_manager,
                watchers,
                tunnel_finder,
                index,
                self.slides.len(),
            )
        })
    }

    pub fn receive_alarm<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        leaderboard: &mut Leaderboard,
        watchers: &Watchers,
        team_manager: Option<&TeamManager>,
        schedule_message: &mut S,
        tunnel_finder: F,
        message: AlarmMessage,
        index: usize,
    ) -> bool {
        let len = self.len();

        if let Some(slide) = self.slides.get_mut(index) {
            slide.receive_alarm(
                leaderboard,
                watchers,
                team_manager,
                schedule_message,
                tunnel_finder,
                message,
                index,
                len,
            )
        } else {
            false
        }
    }
}

impl Slide {
    pub fn play<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        watchers: &Watchers,
        schedule_message: S,
        tunnel_finder: F,
        index: usize,
        count: usize,
    ) {
        match self {
            Self::MultipleChoice(s) => {
                s.play(watchers, schedule_message, tunnel_finder, index, count);
            }
        }
    }

    pub fn receive_message<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        leaderboard: &mut Leaderboard,
        watchers: &Watchers,
        team_manager: Option<&TeamManager>,
        schedule_message: S,
        watcher_id: Id,
        tunnel_finder: F,
        message: IncomingMessage,
        index: usize,
        count: usize,
    ) -> bool {
        match self {
            Self::MultipleChoice(s) => s.receive_message(
                watcher_id,
                message,
                leaderboard,
                watchers,
                team_manager,
                schedule_message,
                tunnel_finder,
                index,
                count,
            ),
        }
    }

    pub fn state_message<T: Tunnel, F: Fn(Id) -> Option<T>>(
        &self,
        watcher_id: Id,
        watcher_kind: ValueKind,
        team_manager: Option<&TeamManager>,
        watchers: &Watchers,
        tunnel_finder: F,
        index: usize,
        count: usize,
    ) -> SyncMessage {
        match self {
            Self::MultipleChoice(s) => SyncMessage::MultipleChoice(s.state_message(
                watcher_id,
                watcher_kind,
                team_manager,
                watchers,
                tunnel_finder,
                index,
                count,
            )),
        }
    }

    fn receive_alarm<
        T: Tunnel,
        F: Fn(Id) -> Option<T>,
        S: FnMut(AlarmMessage, web_time::Duration) -> (),
    >(
        &mut self,
        leaderboard: &mut Leaderboard,
        watchers: &Watchers,
        team_manager: Option<&TeamManager>,
        schedule_message: &mut S,
        tunnel_finder: F,
        message: AlarmMessage,
        index: usize,
        count: usize,
    ) -> bool {
        match self {
            Self::MultipleChoice(s) => s.receive_alarm(
                leaderboard,
                watchers,
                team_manager,
                schedule_message,
                tunnel_finder,
                message,
                index,
                count,
            ),
        }
    }
}
