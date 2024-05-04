use garde::Validate;
use serde::{Deserialize, Serialize};

/// Represents any kinda of media, currently only images
#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub enum Media {
    Image(#[garde(dive)] Image),
}

const CORKBOARD_CONFIG: crate::config::fuiz::corkboard::CorkboardConfig =
    crate::CONFIG.fuiz.corkboard;

const ID_LENGTH: usize = CORKBOARD_CONFIG.id_length.unsigned_abs() as usize;
const MAX_ALT_LENGTH: usize = CORKBOARD_CONFIG.max_alt_length.unsigned_abs() as usize;

#[derive(Debug, Serialize, Deserialize, Clone, Validate)]
pub enum Image {
    Corkboard {
        #[garde(length(min = ID_LENGTH, max = ID_LENGTH))]
        id: String,
        #[garde(length(max = MAX_ALT_LENGTH))]
        alt: String,
    },
}
