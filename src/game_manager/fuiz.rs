use chrono::Duration;
use serde::{Deserialize, Serialize};

use super::{
    media::{Image, Media, TextOrMedia},
    theme::Theme,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct Fuiz {
    title: String,
    description: String,
    thumbnail: Image,
    theme: Theme,
    slides: Vec<Slide>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Slide {
    MultipleChoice(MultipleChoiceSlide),
}

#[serde_with::serde_as]
#[derive(Debug, Serialize, Deserialize)]
pub struct MultipleChoiceSlide {
    title: String,
    media: Option<Media>,
    #[serde_as(as = "serde_with::DurationMilliSeconds<i64>")]
    introduce_question: Duration,
    #[serde_as(as = "serde_with::DurationMilliSeconds<i64>")]
    time_limit: Duration,
    points_awarded: u64,
    answers: Vec<MultipleChoiceAnswer>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MultipleChoiceAnswer {
    correct: bool,
    content: TextOrMedia,
}
