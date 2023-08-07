use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use actix_web::rt::time::Instant;
use atomig::{Atom, Atomic, Ordering};
use dashmap::DashMap;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::game_manager::{game, session::Tunnel};

use super::{
    super::game::{Game, IncomingHostMessage, IncomingMessage, IncomingPlayerMessage},
    config::FuizConfig,
    media::{Media, TextOrMedia},
};

#[derive(Atom, Clone, Copy, Debug, Default)]
#[repr(u8)]
enum SlideState {
    #[default]
    Unstarted,
    Question,
    Answers,
    AnswersResults,
    Leaderboard,
}

#[serde_with::serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Slide {
    title: String,
    media: Option<Media>,
    #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
    introduce_question: Duration,
    #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
    time_limit: Duration,
    points_awarded: u64,
    answers: Vec<Answer>,

    #[serde(skip)]
    user_answers: DashMap<Uuid, (usize, Instant)>,
    #[serde(skip)]
    answer_start: Arc<Mutex<Option<Instant>>>,
    #[serde(skip)]
    slide_state: Arc<Atomic<SlideState>>,
}

#[serde_with::serde_as]
#[derive(Debug, Serialize, Clone)]
pub enum OutcomingMessage {
    QuestionAnnouncment {
        question: String,
        media: Option<Media>,
        #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
        duration: Duration,
    },
    AnswersAnnouncement {
        #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
        duration: Duration,
        answers: Vec<TextOrMedia>,
    },
    AnswersCount(usize),
    AnswersResults {
        answers: Vec<AnswerResult>,
    },
    Leaderboard(Vec<(String, u64)>),
}

impl game::OutcomingMessage for OutcomingMessage {}

#[serde_with::serde_as]
#[derive(Debug, Serialize, Clone)]
pub enum StateMessage {
    QuestionAnnouncment {
        question: String,
        media: Option<Media>,
        #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
        duration: Duration,
    },
    AnswersAnnouncement {
        question: String,
        media: Option<Media>,
        #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
        duration: Duration,
        answers: Vec<TextOrMedia>,
    },
    AnswersResults {
        question: String,
        media: Option<Media>,
        answers: Vec<(TextOrMedia, AnswerResult)>,
    },
    Leaderboard(Vec<(String, u64)>),
}

impl game::StateMessage for StateMessage {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Answer {
    pub correct: bool,
    content: TextOrMedia,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnswerResult {
    correct: bool,
    count: usize,
}

impl Slide {
    pub async fn play<T: Tunnel>(
        &self,
        game: &Game<T>,
        _fuiz: &FuizConfig,
        _index: usize,
        _slides_count: usize,
    ) {
        self.send_question_announcements(game).await;
    }

    fn calculate_score(
        full_duration: Duration,
        taken_duration: Duration,
        full_points_awarded: u64,
    ) -> u64 {
        (taken_duration.as_millis() / 2 * (full_points_awarded as u128) / full_duration.as_millis())
            as u64
    }

    fn start_timer(&self) {
        if let Ok(mut instant) = self.answer_start.lock() {
            *instant = Some(Instant::now());
        }
    }

    fn timer(&self) -> Instant {
        self.answer_start
            .lock()
            .ok()
            .and_then(|x| *x)
            .unwrap_or(Instant::now())
    }

    async fn send_question_announcements<T: Tunnel>(&self, game: &Game<T>) {
        if self.change_state(SlideState::Unstarted, SlideState::Question) {
            self.start_timer();

            game.announce(OutcomingMessage::QuestionAnnouncment {
                question: self.title.clone(),
                media: self.media.clone(),
                duration: self.introduce_question,
            })
            .await;

            actix_web::rt::time::sleep(self.introduce_question).await;

            self.send_answers_announcements(game).await;
        }
    }

    async fn send_answers_announcements<T: Tunnel>(&self, game: &Game<T>) {
        if self.change_state(SlideState::Question, SlideState::Answers) {
            self.start_timer();

            game.announce(OutcomingMessage::AnswersAnnouncement {
                duration: self.time_limit,
                answers: self
                    .answers
                    .iter()
                    .map(|a| a.content.to_owned())
                    .collect_vec(),
            })
            .await;

            actix_web::rt::time::sleep(self.time_limit).await;

            self.send_answers_results(game).await;
        }
    }

    fn change_state(&self, before: SlideState, after: SlideState) -> bool {
        self.slide_state
            .compare_exchange(before, after, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn state(&self) -> SlideState {
        self.slide_state.load(Ordering::SeqCst)
    }

    async fn send_answers_results<T: Tunnel>(&self, game: &Game<T>) {
        if self.change_state(SlideState::Answers, SlideState::AnswersResults) {
            let answer_count = self.user_answers.iter().map(|ua| ua.value().0).counts();
            game.announce(OutcomingMessage::AnswersResults {
                answers: self
                    .answers
                    .iter()
                    .enumerate()
                    .map(|(i, a)| AnswerResult {
                        correct: a.correct,
                        count: *answer_count.get(&i).unwrap_or(&0),
                    })
                    .collect_vec(),
            })
            .await;
        }
    }

    async fn send_leaderboard<T: Tunnel>(&self, game: &Game<T>) {
        if self.change_state(SlideState::AnswersResults, SlideState::Leaderboard) {
            self.add_scores(game);

            game.announce(OutcomingMessage::Leaderboard(game.leaderboard()))
                .await;
        }
    }

    fn add_scores<T: Tunnel>(&self, game: &Game<T>) {
        let starting_instant = self.timer();

        for ua in self.user_answers.iter() {
            let id = ua.key();
            let (answer, instant) = *ua.value();
            let correct = self.answers.get(answer).map(|x| x.correct).unwrap_or(false);
            if correct {
                game.leaderboard.add_score(
                    id.to_owned(),
                    Slide::calculate_score(
                        self.time_limit,
                        instant - starting_instant,
                        self.points_awarded,
                    ),
                );
            }
        }
    }

    pub fn state_message<T: Tunnel>(&self, game: &Game<T>) -> StateMessage {
        match self.state() {
            SlideState::Unstarted | SlideState::Question => StateMessage::QuestionAnnouncment {
                question: self.title.to_owned(),
                media: self.media.to_owned(),
                duration: self.introduce_question - self.timer().elapsed(),
            },
            SlideState::Answers => StateMessage::AnswersAnnouncement {
                question: self.title.to_owned(),
                media: self.media.to_owned(),
                duration: self.time_limit - self.timer().elapsed(),
                answers: self
                    .answers
                    .iter()
                    .enumerate()
                    .map(|(_, a)| a.content.clone())
                    .collect_vec(),
            },
            SlideState::AnswersResults => {
                let answer_count = self.user_answers.iter().map(|ua| ua.value().0).counts();

                StateMessage::AnswersResults {
                    question: self.title.to_owned(),
                    media: self.media.to_owned(),
                    answers: self
                        .answers
                        .iter()
                        .enumerate()
                        .map(|(i, a)| {
                            (
                                a.content.clone(),
                                AnswerResult {
                                    correct: a.correct,
                                    count: *answer_count.get(&i).unwrap_or(&0),
                                },
                            )
                        })
                        .collect_vec(),
                }
            }
            SlideState::Leaderboard => StateMessage::Leaderboard(game.leaderboard()),
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
        match message {
            IncomingMessage::Host(IncomingHostMessage::Next) => {
                match self.slide_state.load(Ordering::SeqCst) {
                    SlideState::Unstarted => self.send_question_announcements(game).await,
                    SlideState::Question => self.send_answers_announcements(game).await,
                    SlideState::Answers => self.send_answers_results(game).await,
                    SlideState::AnswersResults => self.send_leaderboard(game).await,
                    SlideState::Leaderboard => fuiz.play_slide(game, index + 1).await,
                }
            }
            IncomingMessage::Player(IncomingPlayerMessage::IndexAnswer(v))
                if v < self.answers.len() =>
            {
                self.user_answers.insert(uuid, (v, Instant::now()));
                let left_set: HashSet<_> =
                    game.watchers.players_iter().map(|w| w.key().id).collect();
                let right_set: HashSet<_> = self
                    .user_answers
                    .iter()
                    .map(|ua| ua.key().to_owned())
                    .collect();
                if left_set.is_subset(&right_set) {
                    self.send_answers_results(game).await;
                } else {
                    game.announce(OutcomingMessage::AnswersCount(
                        left_set.intersection(&right_set).count(),
                    ))
                    .await;
                }
            }
            _ => (),
        }
    }
}
