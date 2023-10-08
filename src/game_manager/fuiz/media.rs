use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Media {
    Image(Image),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum Image {
    Corkboard { id: String, alt: String },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TextOrMedia {
    Media(Media),
    Text(String),
}
