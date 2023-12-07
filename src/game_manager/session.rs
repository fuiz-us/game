use actix_ws::Closed;
use async_trait::async_trait;

#[derive(Clone)]
pub struct Session {
    session: actix_ws::Session,
}

#[async_trait]
pub trait Tunnel: Clone {
    async fn send(&self, message: &str) -> Result<(), Closed>;

    fn close(self);
}

impl Session {
    pub fn new(session: actix_ws::Session) -> Self {
        Self { session }
    }
}

#[async_trait]
impl Tunnel for Session {
    async fn send(&self, message: &str) -> Result<(), Closed> {
        let mut session = self.session.clone();

        session.text(message).await
    }

    fn close(self) {
        actix_web::rt::spawn(async move {
            let _ = self.session.close(None).await;
        });
    }
}
