pub mod config;
pub mod connection;
pub mod operations;
pub mod transport;

use self::transport::Transport;
use crate::client::config::{ClientConfig, LoginCredentials};
use crate::client::connection::{CErr, Connection, ConnectionIncomingMessages};
use crate::message::IRCMessage;
use crate::util::MutexAndGuard;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use futures::stream::StreamExt;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::Mutex;

struct ClientOutgoing<T: Transport, L: LoginCredentials> {
    channels: Mutex<HashSet<String>>,
    connections: Mutex<VecDeque<Arc<Mutex<Connection<T>>>>>,
    config: Arc<ClientConfig<L>>,
    kill_join_loop_tx: Mutex<Option<oneshot::Sender<()>>>,

    incoming_messages_tx: mpsc::Sender<IRCMessage>,
    join_queue_tx: mpsc::UnboundedSender<String>,
}

impl<T: Transport, L: LoginCredentials> ClientOutgoing<T, L> {
    fn on_new_connection(
        &self,
        conn: Arc<Mutex<Connection<T>>>,
        mut incoming_messages: ConnectionIncomingMessages<T, L>,
    ) {
        let mut incoming_messages_tx = self.incoming_messages_tx.clone();
        let mut join_queue_tx = self.join_queue_tx.clone();
        tokio::spawn(async move {
            while let Some(msg_or_err) = incoming_messages.next().await {
                match msg_or_err {
                    Ok(msg) => {
                        let maybe_send_err = incoming_messages_tx.send(msg).await;
                        if maybe_send_err.is_err() {
                            log::info!("ConnectionIncoming -> ClientIncoming forwarder ending: Receiver dropped");
                            break;
                        }
                    }
                    Err(err) => {
                        log::info!("ConnectionIncoming -> ClientIncoming forwarder ending: Encountered incoming error (rejoin will be scheduled)\nError is: {}", err);
                        let conn = conn.lock().await;
                        for channel in conn.channels.iter() {
                            // ok(): Ignore if the loop task has ended (whole client was closed)
                            join_queue_tx.send(channel.clone()).await.ok();
                        }
                        break;
                    }
                }
            }
        });
    }

    async fn maybe_pick_connection<F: FnMut(&mut Connection<T>) -> bool>(
        &self,
        mut matcher: F,
        create_new_conn: bool,
    ) -> Option<MutexAndGuard<Connection<T>>> {
        let mut connections = self.connections.lock().await;

        let mut misses = VecDeque::new();

        let mut winner = None;
        while let Some(connection) = connections.pop_front() {
            let mut connection_locked = MutexAndGuard::async_new(Arc::clone(&connection)).await;
            if matcher(&mut connection_locked) {
                winner = Some((connection, connection_locked));
                break;
            } else {
                misses.push_back(connection);
            }
        }

        while let Some(e) = misses.pop_back() {
            connections.push_front(e);
        }

        if let Some((winner, winner_locked)) = winner {
            connections.push_back(Arc::clone(&winner));
            Some(winner_locked)
        } else if create_new_conn {
            // new connection has to be made
            let (new_conn, incoming_messages) = Connection::<T>::new(self.config.clone());
            connections.push_back(Arc::clone(&new_conn));

            // unlock Mutex, THEN await so we don't block the mutex longer than needed.
            drop(connections);
            self.on_new_connection(Arc::clone(&new_conn), incoming_messages);
            Some(MutexAndGuard::async_new(new_conn).await)
        } else {
            None
        }
    }

    async fn pick_connection<F: FnMut(&mut Connection<T>) -> bool>(
        &self,
        matcher: F,
    ) -> MutexAndGuard<Connection<T>> {
        self.maybe_pick_connection(matcher, true).await.unwrap()
    }

    pub async fn send_msg(&self, msg: IRCMessage) -> Result<(), CErr<T, L>> {
        let mut conn = self.pick_connection(|_| true).await;
        conn.send_msg::<L>(msg).await
    }

    // TRY to join the channel. this is a non-public function called by the join loop
    async fn join(&self, channel: String) -> Result<(), CErr<T, L>> {
        let mut channels = self.channels.lock().await;
        // TODO i need to figure out a clean way of doing this
        let was_added = channels.insert(channel.clone());
        if was_added {
            let mut conn = self.pick_connection(|conn| conn.channels.len() < 50).await;
            conn.join::<L>(channel).await?;
        }
        Ok(())
    }

    // public method
    async fn part(&self, channel: &str) -> Result<(), CErr<T, L>> {
        // todo as well
        let was_removed = self.channels.lock().await.remove(channel);
        if was_removed {
            let conn = self
                .maybe_pick_connection(|conn| conn.channels.contains(channel), false)
                .await;
            if let Some(mut conn) = conn {
                conn.part::<L>(channel).await?;
            }
        }

        Ok(())
    }
}

struct ClientJoinLoop<T: Transport, L: LoginCredentials> {
    // TODO will this cause a reference loop with ConnectionOutgoing holding a join_queue_tx?
    outgoing: Arc<ClientOutgoing<T, L>>,
    join_queue_rx: stream::Fuse<mpsc::UnboundedReceiver<String>>,
    join_queue_tx: mpsc::UnboundedSender<String>,
    kill_join_loop_rx: future::Fuse<oneshot::Receiver<()>>,
}

impl<T: Transport, L: LoginCredentials> ClientJoinLoop<T, L> {
    fn on_recv_channel_to_join(&self, channel: String) {
        let outgoing = Arc::clone(&self.outgoing);
        let mut join_queue_tx = mpsc::UnboundedSender::clone(&self.join_queue_tx);
        tokio::spawn(async move {
            let maybe_err = outgoing.join(channel.clone()).await;

            if maybe_err.is_err() {
                // ok(): If the channel is closed (because the client was closed)
                join_queue_tx.send(channel).await.ok();
            }
        });
    }

    async fn start(mut self) {
        loop {
            futures::select_biased! {
                recv_result = (&mut self.kill_join_loop_rx) => {
                    if recv_result.is_err() {
                        log::warn!("ClientOutgoing was dropped before sending kill signal to ClientJoinLoop");
                    } else {
                        log::info!("ClientJoinLoop task ending: Received kill signal by ClientOutgoing")
                    }

                    break;
                },
                recv_result = self.join_queue_rx.next() => {
                    match recv_result {
                        Some(channel) => self.on_recv_channel_to_join(channel),
                        None => {
                            // this can theoretically happen if kill_join_queue_rx is not triggered
                            // and all sending halves get dropped (i.e. all tasks spawned by
                            // on_recv_channel_to_join have ended + ConnectionOutgoing dropped)
                            log::info!("ClientJoinLoop task ending: end of join_queue_rx");
                            break;
                        }
                    }
                },
            }
        }

        log::info!("ClientJoinLoop ending");
    }
}

pub struct TwitchIRCClient<T: Transport, L: LoginCredentials> {
    // Option<> so you can take() it to own it
    pub incoming_messages: Option<mpsc::Receiver<IRCMessage>>,

    outgoing: Arc<ClientOutgoing<T, L>>,
    join_queue_tx: mpsc::UnboundedSender<String>,
}

impl<T: Transport, L: LoginCredentials> TwitchIRCClient<T, L> {
    pub fn new(config: ClientConfig<L>) -> TwitchIRCClient<T, L> {
        let config = Arc::new(config);

        let (kill_join_loop_tx, kill_join_loop_rx) = oneshot::channel();
        let (incoming_messages_tx, incoming_messages_rx) = mpsc::channel(16);
        let (join_queue_tx, join_queue_rx) = mpsc::unbounded();

        let outgoing = Arc::new(ClientOutgoing {
            connections: Mutex::new(VecDeque::new()),
            channels: Mutex::new(HashSet::new()),
            config,
            kill_join_loop_tx: Mutex::new(Some(kill_join_loop_tx)),
            incoming_messages_tx,
            join_queue_tx: join_queue_tx.clone(),
        });

        let join_loop = ClientJoinLoop {
            outgoing: outgoing.clone(),
            join_queue_tx: join_queue_tx.clone(),
            join_queue_rx: join_queue_rx.fuse(),
            kill_join_loop_rx: kill_join_loop_rx.fuse(),
        };
        tokio::spawn(join_loop.start());

        TwitchIRCClient {
            incoming_messages: Some(incoming_messages_rx),
            join_queue_tx,
            outgoing,
        }
    }

    pub fn take_incoming_messages(&mut self) -> Option<mpsc::Receiver<IRCMessage>> {
        self.incoming_messages.take()
    }

    pub async fn send_msg(&self, msg: IRCMessage) -> Result<(), CErr<T, L>> {
        self.outgoing.send_msg(msg).await
    }

    pub async fn join(&self, channel: String) {
        self.join_queue_tx
            .clone()
            .send(channel)
            .await
            .expect("join() called after TwitchIRCClient was closed");
    }

    pub async fn part(&self, channel: &str) {
        // TODO shit
        self.outgoing.part(channel).await.ok();
    }

    pub async fn close(&self) {
        todo!()
    }
}
