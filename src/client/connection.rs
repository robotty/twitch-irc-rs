use super::transport::Transport;
use crate::client::config::{ClientConfig, LoginCredentials};
use crate::client::operations::{ConnectionOperations, LoginError};
use crate::message::IRCMessage;
use futures::channel::mpsc::Sender;
use futures::prelude::*;
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct Connection<T: Transport, L: LoginCredentials> {
    pub incoming_messages: Option<T::Incoming>,
    pub outgoing_messages: Arc<Mutex<T::Outgoing>>,
    pub channels: Mutex<HashSet<String>>,
    pub config: Arc<ClientConfig<L>>,
}

impl<T: Transport, L: LoginCredentials> Connection<T, L> {
    pub fn new(transport: T, config: Arc<ClientConfig<L>>) -> Connection<T, L> {
        // destructure the given transport...
        let (incoming_messages, outgoing_messages) = transport.split();

        // and build a Connection from the parts
        Connection {
            incoming_messages: Some(incoming_messages),
            outgoing_messages: Arc::new(Mutex::new(outgoing_messages)),
            channels: Mutex::new(HashSet::new()),
            config,
        }
    }

    pub async fn initialize(&self) -> Result<(), LoginError<L::Error, T::OutgoingError>> {
        //let outgoing_messages = self.outgoing_messages.lock().await;
        // TODO this is a data race with other things also sending messages at connection startup
        //  we would ideally need a re-entrant mutex

        self.request_capabilities()
            .await
            .map_err(LoginError::TransportOutgoingError)?;
        self.login().await?;

        Ok(())
    }

    pub fn run_forwarder(
        &mut self,
        mut sender: Sender<Result<IRCMessage, T::IncomingError>>,
    ) -> impl Future<Output = ()> {
        let mut incoming_messages = self.incoming_messages.take().unwrap();

        async move {
            while let Some(message) = incoming_messages.next().await {
                let res = sender.send(message).await;
                if let Err(send_error) = res {
                    if send_error.is_disconnected() {
                        break;
                    } else {
                        panic!("unexpected send error")
                    }
                }
            }
        }
    }
}
