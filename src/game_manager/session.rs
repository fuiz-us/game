use std::sync::Mutex;

use actix_ws::Closed;
use async_trait::async_trait;

#[cfg(test)]
use mockall::automock;

pub struct Session {
    session: Mutex<actix_ws::Session>,
}

#[cfg_attr(test, automock)]
#[async_trait]
pub trait Tunnel {
    async fn send(&self, message: &str) -> Result<(), Closed>;
}

impl Session {
    pub fn new(session: actix_ws::Session) -> Self {
        Self {
            session: Mutex::new(session),
        }
    }
}

#[async_trait]
impl Tunnel for Session {
    async fn send(&self, message: &str) -> Result<(), Closed> {
        // This avoids holding the mutex lock while it awaits sending the message
        let session = match self.session.lock() {
            Ok(s) => Ok(s.clone()),
            Err(_) => Err(Closed),
        };

        match session {
            Ok(mut session) => session.text(message).await,
            Err(c) => Err(c),
        }
    }
}
