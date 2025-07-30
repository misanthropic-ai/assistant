use ractor::{Actor, ActorRef};
use crate::messages::ChatMessage;

pub mod cli;

/// Trait for display actors that handle output formatting
pub trait DisplayActor: Actor<Msg = ChatMessage> {
    /// Get the name of this display type
    fn display_type(&self) -> &'static str;
}