use std::{fmt::Display, num::ParseIntError, str::FromStr};

use enum_map::{Enum, EnumArray};
use serde::{Deserialize, Deserializer, Serialize};

const MIN_VALUE: u16 = 0o10_000;
const MAX_VALUE: u16 = 0o100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GameId(u16);

impl GameId {
    pub fn new() -> Self {
        Self(fastrand::u16(MIN_VALUE..MAX_VALUE))
    }
}

impl Default for GameId {
    fn default() -> Self {
        Self::new()
    }
}

impl Display for GameId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:05o}", self.0)
    }
}

impl Serialize for GameId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for GameId {
    fn deserialize<D>(deserializer: D) -> Result<GameId, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        GameId::from_str(&s).map_err(|e| serde::de::Error::custom(e.to_string()))
    }
}

impl FromStr for GameId {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(u16::from_str_radix(s, 8)?))
    }
}

impl Enum for GameId {
    const LENGTH: usize = (MAX_VALUE - MIN_VALUE) as usize;

    fn from_usize(value: usize) -> Self {
        Self(u16::try_from(value).expect("index out of range for Enum::from_usize") + MIN_VALUE)
    }

    fn into_usize(self) -> usize {
        usize::from(self.0.saturating_sub(MIN_VALUE)).min(GameId::LENGTH - 1)
    }
}

impl<V> EnumArray<V> for GameId {
    type Array = [V; Self::LENGTH];
}
