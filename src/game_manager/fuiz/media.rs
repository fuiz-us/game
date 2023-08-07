use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Media {
    Image(Image),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Image {
    Internet(InternetImage),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InternetImage {
    pub url: String,
    pub alt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TextOrMedia {
    Media(Media),
    Text(String),
}
