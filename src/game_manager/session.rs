use super::{SyncMessage, UpdateMessage};

#[derive(Clone)]
pub struct Session {
    session: actix_ws::Session,
}

// pub enum Message {
//     Outgoing(OutgoingMessage),
//     State(StateMessage),
// }

pub trait Tunnel: Clone {
    fn send_message(&self, message: &UpdateMessage);

    fn send_state(&self, state: &SyncMessage);

    // fn send_multiple(&self, messages: &[Message]);

    fn close(self);
}

impl Session {
    pub fn new(session: actix_ws::Session) -> Self {
        Self { session }
    }
}

impl Tunnel for Session {
    fn send_message(&self, message: &UpdateMessage) {
        let mut session = self.session.clone();

        let message = message.to_message();

        actix_web::rt::spawn(async move {
            let _ = session.text(message).await;
        });
    }

    fn send_state(&self, state: &SyncMessage) {
        let mut session = self.session.clone();

        let message = state.to_message();

        actix_web::rt::spawn(async move {
            let _ = session.text(message).await;
        });
    }

    // fn send_multiple(&self, messages: &[Message]) {
    //     let mut session = self.session.clone();

    //     let messages = messages.into_iter().map(|m| match m {
    //         Message::Outgoing(o) => o.to_message(),
    //         Message::State(s) => s.to_message()
    //     }).collect_vec();

    //     actix_web::rt::spawn(async move {
    //         for message in messages {
    //             if session.text(message).await.is_err() {
    //                 return;
    //             }
    //         }
    //     });
    // }

    fn close(self) {
        actix_web::rt::spawn(async move {
            let _ = self.session.close(None).await;
        });
    }
}
