use super::TransportError;
use crate::message::IRCMessage;
use futures::stream::Stream;
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project]
pub struct MessageStream<S: Stream<Item = std::io::Result<String>>> {
    #[pin]
    source: S,
}

impl<S: Stream<Item = std::io::Result<String>>> MessageStream<S> {
    pub fn new(source: S) -> MessageStream<S> {
        MessageStream { source }
    }
}

impl<S: Stream<Item = std::io::Result<String>>> Stream for MessageStream<S> {
    type Item = Result<IRCMessage, TransportError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.project()
            .source
            .poll_next(cx)
            .map(|opt| opt.map(|res| Ok(IRCMessage::parse(&res?)?)))
    }
}
