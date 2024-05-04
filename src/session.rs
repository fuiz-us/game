use super::{SyncMessage, UpdateMessage};

// pub enum Message {
//     Outgoing(OutgoingMessage),
//     State(StateMessage),
// }

pub trait Tunnel {
    fn send_message(&self, message: &UpdateMessage);

    fn send_state(&self, state: &SyncMessage);

    // fn send_multiple(&self, messages: &[Message]);

    fn close(self);
}
