use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum Media {
    Image(Image),
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Image {
    Internet(InternetImage),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InternetImage {
    url: String,
    alt: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum TextOrMedia {
    Media(Media),
    Text(String),
}