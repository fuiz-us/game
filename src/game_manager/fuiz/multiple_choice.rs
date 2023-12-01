use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Duration,
};

use actix_web::rt::time::Instant;
use atomig::{Atom, Atomic, Ordering};
use dashmap::DashMap;
use garde::Validate;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::game_manager::{
    game,
    session::Tunnel,
    watcher::{WatcherId, WatcherValueKind},
};

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
}

type ValidationResult = garde::Result;

fn validate_duration<const MIN_SECONDS: u64, const MAX_SECONDS: u64>(
    field: &'static str,
    val: &Duration,
) -> ValidationResult {
    match (MIN_SECONDS..=MAX_SECONDS).contains(&val.as_secs()) {
        true => Ok(()),
        false => Err(garde::Error::new(format!(
            "{field} is outside of the bounds [{MIN_SECONDS},{MAX_SECONDS}]",
        ))),
    }
}

const CONFIG: crate::config::fuiz::multiple_choice::MultipleChoiceConfig =
    crate::CONFIG.fuiz.multiple_choice;

const MIN_TITLE_LENGTH: usize = CONFIG.min_title_length.unsigned_abs() as usize;
const MIN_INTRODUCE_QUESTION: u64 = CONFIG.min_introduce_question.unsigned_abs();
const MIN_TIME_LIMIT: u64 = CONFIG.min_time_limit.unsigned_abs();

const MAX_TIME_LIMIT: u64 = CONFIG.max_time_limit.unsigned_abs();
const MAX_TITLE_LENGTH: usize = CONFIG.max_title_length.unsigned_abs() as usize;
const MAX_INTRODUCE_QUESTION: u64 = CONFIG.max_introduce_question.unsigned_abs();

const MAX_ANSWER_COUNT: usize = CONFIG.max_answer_count.unsigned_abs() as usize;

fn validate_introduce_question(val: &Duration) -> ValidationResult {
    validate_duration::<MIN_INTRODUCE_QUESTION, MAX_INTRODUCE_QUESTION>("introduce_question", val)
}

fn validate_time_limit(val: &Duration) -> ValidationResult {
    validate_duration::<MIN_TIME_LIMIT, MAX_TIME_LIMIT>("time_limit", val)
}

#[serde_with::serde_as]
#[derive(Debug, Clone, Default, Serialize, serde::Deserialize, Validate)]
pub struct Slide {
    #[garde(length(min = MIN_TITLE_LENGTH, max = MAX_TITLE_LENGTH))]
    title: String,
    #[garde(dive)]
    media: Option<Media>,
    #[garde(custom(|v, _| validate_introduce_question(v)))]
    #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
    introduce_question: Duration,
    #[garde(custom(|v, _| validate_time_limit(v)))]
    #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
    time_limit: Duration,
    #[garde(skip)]
    points_awarded: u64,
    #[garde(length(max = MAX_ANSWER_COUNT))]
    answers: Vec<AnswerChoice>,

    #[serde(skip)]
    #[garde(skip)]
    user_answers: DashMap<WatcherId, (usize, Instant)>,
    #[serde(skip)]
    #[garde(skip)]
    answer_start: Arc<Mutex<Option<Instant>>>,
    #[serde(skip)]
    #[garde(skip)]
    slide_state: Arc<Atomic<SlideState>>,
}

#[serde_with::serde_as]
#[skip_serializing_none]
#[derive(Debug, Serialize, Clone)]
pub enum OutgoingMessage {
    QuestionAnnouncment {
        index: usize,
        count: usize,
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
        results: Vec<AnswerChoiceResult>,
    },
}

impl game::OutgoingMessage for OutgoingMessage {
    fn identifier(&self) -> &'static str {
        "MultipleChoice"
    }
}

#[serde_with::serde_as]
#[skip_serializing_none]
#[derive(Debug, Serialize, Clone)]
pub enum StateMessage {
    QuestionAnnouncment {
        index: usize,
        count: usize,
        question: String,
        media: Option<Media>,
        #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
        duration: Duration,
    },
    AnswersAnnouncement {
        index: usize,
        count: usize,
        question: String,
        media: Option<Media>,
        #[serde_as(as = "serde_with::DurationMilliSeconds<u64>")]
        duration: Duration,
        answers: Vec<TextOrMedia>,
        answered_count: usize,
    },
    AnswersResults {
        index: usize,
        count: usize,
        question: String,
        media: Option<Media>,
        answers: Vec<TextOrMedia>,
        results: Vec<AnswerChoiceResult>,
    },
}

impl game::StateMessage for StateMessage {
    fn identifier(&self) -> &'static str {
        "MultipleChoice"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerChoice {
    pub correct: bool,
    pub content: TextOrMedia,
}

#[derive(Debug, Serialize, Clone)]
pub struct AnswerChoiceResult {
    correct: bool,
    count: usize,
}

impl Slide {
    pub async fn play<T: Tunnel>(
        &self,
        game: &Game<T>,
        _fuiz: &FuizConfig,
        index: usize,
        count: usize,
    ) {
        self.send_question_announcements(game, index, count).await;
    }

    fn calculate_score(
        full_duration: Duration,
        taken_duration: Duration,
        full_points_awarded: u64,
    ) -> u64 {
        full_points_awarded
            - ((taken_duration.as_millis() / 2 * (full_points_awarded as u128)
                / full_duration.as_millis()) as u64)
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

    async fn send_question_announcements<T: Tunnel>(
        &self,
        game: &Game<T>,
        index: usize,
        count: usize,
    ) {
        if self.change_state(SlideState::Unstarted, SlideState::Question) {
            self.start_timer();

            game.announce(OutgoingMessage::QuestionAnnouncment {
                index,
                count,
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

            game.announce(OutgoingMessage::AnswersAnnouncement {
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
            game.announce(OutgoingMessage::AnswersResults {
                results: self
                    .answers
                    .iter()
                    .enumerate()
                    .map(|(i, a)| AnswerChoiceResult {
                        correct: a.correct,
                        count: *answer_count.get(&i).unwrap_or(&0),
                    })
                    .collect_vec(),
            })
            .await;
        }
    }

    fn add_scores<T: Tunnel>(&self, game: &Game<T>) {
        let starting_instant = self.timer();

        for ua in self.user_answers.iter() {
            let id = ua.key();
            let (answer, instant) = *ua.value();
            let correct = self.answers.get(answer).map(|x| x.correct).unwrap_or(false);
            game.leaderboard.add_score(id.to_owned(), {
                if correct {
                    Slide::calculate_score(
                        self.time_limit,
                        instant - starting_instant,
                        self.points_awarded,
                    )
                } else {
                    0
                }
            });
        }
    }

    pub fn state_message<T: Tunnel>(
        &self,
        _watcher_id: WatcherId,
        _watcher_kind: WatcherValueKind,
        game: &Game<T>,
        index: usize,
        count: usize,
    ) -> StateMessage {
        match self.state() {
            SlideState::Unstarted | SlideState::Question => StateMessage::QuestionAnnouncment {
                index,
                count,
                question: self.title.to_owned(),
                media: self.media.to_owned(),
                duration: self.introduce_question - self.timer().elapsed(),
            },
            SlideState::Answers => StateMessage::AnswersAnnouncement {
                index,
                count,
                question: self.title.to_owned(),
                media: self.media.to_owned(),
                duration: self.time_limit - self.timer().elapsed(),
                answers: self
                    .answers
                    .iter()
                    .enumerate()
                    .map(|(_, a)| a.content.clone())
                    .collect_vec(),
                answered_count: {
                    let left_set: HashSet<_> = game
                        .watchers
                        .specific_vec(WatcherValueKind::Player)
                        .iter()
                        .map(|(w, _, _)| w.to_owned())
                        .collect();
                    let right_set: HashSet<_> = self
                        .user_answers
                        .iter()
                        .map(|ua| ua.key().to_owned())
                        .collect();
                    left_set.intersection(&right_set).count()
                },
            },
            SlideState::AnswersResults => {
                let answer_count = self.user_answers.iter().map(|ua| ua.value().0).counts();

                StateMessage::AnswersResults {
                    index,
                    count,
                    question: self.title.to_owned(),
                    media: self.media.to_owned(),
                    answers: self.answers.iter().map(|a| a.content.clone()).collect_vec(),
                    results: self
                        .answers
                        .iter()
                        .enumerate()
                        .map(|(i, a)| AnswerChoiceResult {
                            correct: a.correct,
                            count: *answer_count.get(&i).unwrap_or(&0),
                        })
                        .collect_vec(),
                }
            }
        }
    }

    pub async fn receive_message<T: Tunnel>(
        &self,
        game: &Game<T>,
        _fuiz: &FuizConfig,
        watcher_id: WatcherId,
        message: IncomingMessage,
        index: usize,
        count: usize,
    ) {
        match message {
            IncomingMessage::Host(IncomingHostMessage::Next) => {
                match self.slide_state.load(Ordering::SeqCst) {
                    SlideState::Unstarted => {
                        self.send_question_announcements(game, index, count).await
                    }
                    SlideState::Question => self.send_answers_announcements(game).await,
                    SlideState::Answers => self.send_answers_results(game).await,
                    SlideState::AnswersResults => {
                        self.add_scores(game);
                        game.finish_slide().await;
                    }
                }
            }
            IncomingMessage::Player(IncomingPlayerMessage::IndexAnswer(v))
                if v < self.answers.len() =>
            {
                self.user_answers.insert(watcher_id, (v, Instant::now()));
                let left_set: HashSet<_> = game
                    .watchers
                    .specific_vec(WatcherValueKind::Player)
                    .iter()
                    .map(|(w, _, _)| w.to_owned())
                    .collect();
                let right_set: HashSet<_> = self
                    .user_answers
                    .iter()
                    .map(|ua| ua.key().to_owned())
                    .collect();
                if left_set.is_subset(&right_set) {
                    self.send_answers_results(game).await;
                } else {
                    game.announce_host(OutgoingMessage::AnswersCount(
                        left_set.intersection(&right_set).count(),
                    ))
                    .await;
                }
            }
            _ => (),
        }
    }
}
