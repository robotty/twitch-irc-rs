use super::transport::Transport;
use crate::client::config::{ClientConfig, LoginCredentials};
use crate::irc;
use crate::message::AsRawIRC;
use crate::message::IRCMessage;
use crate::util::MutexAndGuard;
use futures::channel::{mpsc, oneshot};
use futures::future;
use futures::prelude::*;
use std::collections::HashSet;
use std::fmt::{Debug, Display};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Error, Debug)]
pub enum ConnectionError<TC, TI, TO, L>
where
    TC: Send + Sync + Display + Debug,
    TI: Send + Sync + Display + Debug,
    TO: Send + Sync + Display + Debug,
    L: Send + Sync + Display + Debug,
{
    #[error("{0}")]
    ConnectError(TC),
    #[error("{0}")]
    IncomingError(TI),
    #[error("{0}")]
    OutgoingError(TO),
    #[error("{0}")]
    LoginError(L),
    #[error("Outgoing messages stream closed")]
    Closed(),
}

pub type CErr<T, L> = ConnectionError<
    <T as Transport>::ConnectError,
    <T as Transport>::IncomingError,
    <T as Transport>::OutgoingError,
    <L as LoginCredentials>::Error,
>;

// ConnectionOutgoing
// Tx has error:
// - Sends signal over tx_kill_incoming immediately
// - Ends itself so no more messages get accepted
//
// Rx has error:
// - rx_kill_outgoing is read from before send_msg is attempted
// - If error or ConnectionIncoming was dropped then ConnectionOutgoing ends itself so no more messages get sent

// ConnectionIncoming
// Tx has error:
// - Signal is received right away (futures::select_biased!)
// - Ends itself right away (reads no more messages, drops the read half of the transport stream)
//
// Rx has error:
// - same procedure as above
//
// Receiving end of Rx is dropped:
// - same procedure as above

struct ConnectionIncoming<T: Transport, L: LoginCredentials> {
    transport_incoming_rx: Option<T::Incoming>,
    connection_incoming_tx: mpsc::Sender<Result<IRCMessage, CErr<T, L>>>,
    rx_kill_incoming: future::Fuse<oneshot::Receiver<()>>,
    tx_kill_outgoing: Option<oneshot::Sender<()>>,
}

impl<T: Transport, L: LoginCredentials> ConnectionIncoming<T, L> {
    fn stop_incoming_and_outgoing(&mut self) {
        // stop ConnectionOutgoing
        if let Some(tx_kill_outgoing) = self.tx_kill_outgoing.take() {
            tx_kill_outgoing.send(()).ok();
        }

        // stop ourselves (end loop)
        drop(self.transport_incoming_rx.take());
    }

    async fn on_message_from_transport(&mut self, message: IRCMessage) {
        log::trace!("< {}", message.as_raw_irc());

        // TODO: ping/pong, RECONNECT, ... here

        let send_err = self.connection_incoming_tx.send(Ok(message)).await;

        if let Err(_) = send_err {
            log::trace!("Rx task ending: receiving end dropped");
            self.stop_incoming_and_outgoing();
        }
    }

    async fn on_error_from_transport(&mut self, error: T::IncomingError) {
        log::info!("Rx task ending: Error while reading from transport");

        // send the error downstream
        // .ok(): If an error occurs here, then the receiving end has been
        // dropped, which is not a condition we need to handle here since
        // we close this Connection anyways
        self.connection_incoming_tx
            .send(Err(CErr::<T, L>::IncomingError(error)))
            .await
            .ok();

        // now stop the loop and the ConnectionOutgoing
        self.stop_incoming_and_outgoing();
    }

    async fn on_init_error(&mut self, error: CErr<T, L>) {
        // TODO update usages of Rx task/incoming task/etc in documentation, comments and log messages. and unify
        log::info!("Rx task will not start (discarding): Initialization failure");

        // .ok(): If an error occurs here, then the receiving end has been
        // dropped, which is not a condition we need to handle here since
        // we close this Connection anyways
        self.connection_incoming_tx.send(Err(error)).await.ok();

        self.stop_incoming_and_outgoing();
    }

    fn on_eof_from_transport(&mut self) {
        log::info!("Rx task ending: EOF while reading from TCP socket");
        self.stop_incoming_and_outgoing();
    }

    async fn start(mut self) {
        // calling self.stop_incoming_and_outgoing() will cause this to break on
        // the next iteration of the loop

        while let Some(transport_incoming_rx) = &mut self.transport_incoming_rx {
            // biased select: We want rx_kill_incoming to take priority.
            futures::select_biased! {
                recv_result = (&mut self.rx_kill_incoming) => {
                    // if result is Ok, then we definitely got command to shut down
                    // if Err, then the sending part got dropped before sending something
                    // (which is not supposed to happen)
                    if recv_result.is_err() {
                        log::warn!("ConnectionOutgoing was dropped before sending kill signal to ConnectionIncoming task")
                    } else {
                        // sender had an error, we need to shut down
                        log::info!("ConnectionIncoming task ending: Received kill signal by ConnectionOutgoing");
                    }
                    self.stop_incoming_and_outgoing();
                },
                message = transport_incoming_rx.next() => {
                    match message {
                        Some(Ok(message)) => {
                            // got a message
                            self.on_message_from_transport(message).await;
                        },
                        Some(Err(error)) => {
                            // stream encounters error
                            self.on_error_from_transport(error).await;
                        },
                        None => {
                            // stream ends without error
                            self.on_eof_from_transport();
                        }
                    }
                },
            }
        }

        log::info!("End of Rx task");
    }
}

// TODO find-replace usages
pub type ConnectionIncomingMessages<T, L> = mpsc::Receiver<Result<IRCMessage, CErr<T, L>>>;

pub struct Connection<T: Transport> {
    pub channels: HashSet<String>,
    transport_outgoing_tx: Option<T::Outgoing>,
    tx_kill_incoming: Option<oneshot::Sender<()>>,
    rx_kill_outgoing: oneshot::Receiver<()>,
}

impl<T: Transport> Connection<T> {
    pub fn new<L: LoginCredentials>(
        config: Arc<ClientConfig<L>>,
    ) -> (Arc<Mutex<Connection<T>>>, ConnectionIncomingMessages<T, L>) {
        let (tx_kill_outgoing, rx_kill_outgoing) = oneshot::channel();
        let (tx_kill_incoming, rx_kill_incoming) = oneshot::channel();
        let (connection_incoming_tx, connection_incoming_rx) = mpsc::channel(16);

        let conn = Arc::new(Mutex::new(Connection {
            channels: HashSet::new(),
            transport_outgoing_tx: None,
            tx_kill_incoming: Some(tx_kill_incoming),
            rx_kill_outgoing,
        }));

        let incoming = ConnectionIncoming {
            transport_incoming_rx: None,
            connection_incoming_tx,
            rx_kill_incoming: rx_kill_incoming.fuse(),
            tx_kill_outgoing: Some(tx_kill_outgoing),
        };
        tokio::spawn(Connection::init_task(incoming, conn.clone(), config));

        (conn, connection_incoming_rx)
    }

    fn init_task<L: LoginCredentials>(
        mut incoming: ConnectionIncoming<T, L>,
        outgoing: Arc<Mutex<Connection<T>>>,
        config: Arc<ClientConfig<L>>,
    ) -> impl Future<Output = ()> + 'static {
        // pack the Mutex and its guard together so the packed-together values can be moved to the
        // async block without getting problems from the borrow checker
        let mut outgoing_locked = MutexAndGuard::new(outgoing, |outgoing| {
            outgoing
                .try_lock()
                .expect("init_task called while outgoing is locked")
        });

        async move {
            // we need both the "get login token" and "connect transport" to succeed
            // so this is grouped to handle both errors at once
            let maybe_transport_and_login_token: Result<(T, Option<String>), CErr<T, L>> = async {
                let token = config
                    .login_credentials
                    .get_token()
                    .await
                    .map_err(ConnectionError::LoginError)?
                    .clone();
                let transport = T::new().await.map_err(ConnectionError::ConnectError)?;
                Ok((transport, token))
            }
            .await;

            // if either there is no login token or the connection failed to connect then we have to abort
            let (transport, token) = match maybe_transport_and_login_token {
                Err(init_error) => {
                    // closes the contained channel too, so a downstream reading
                    // from the incoming_messages will end
                    incoming.on_init_error(init_error).await;

                    // exit the init task
                    return;
                }
                Ok(e) => e,
            };

            let (transport_incoming_rx, transport_outgoing_tx) = transport.split();

            incoming.transport_incoming_rx = Some(transport_incoming_rx);
            outgoing_locked.transport_outgoing_tx = Some(transport_outgoing_tx);

            let login = config.login_credentials.get_login().to_owned();

            // Start ConnectionIncoming task
            tokio::spawn(incoming.start());

            // initialize the IRC connection with setup commands
            outgoing_locked
                .send_msg::<L>(irc!["CAP", "REQ", "twitch.tv/commands twitch.tv/tags"])
                .await
                .ok();
            if let Some(token) = token {
                outgoing_locked
                    .send_msg::<L>(irc!["PASS", format!("oauth:{}", token)])
                    .await
                    .ok();
            }
            outgoing_locked
                .send_msg::<L>(irc!["NICK", login])
                .await
                .ok();

            // unlock the mutex, now allow messages to come in from the outside
            drop(outgoing_locked);
        }
    }

    fn stop_incoming_and_outgoing(&mut self) {
        // tell the ConnectionIncoming to stop
        if let Some(tx_kill_incoming) = self.tx_kill_incoming.take() {
            // .ok(): Ignore if ConnectionIncoming has already ended
            tx_kill_incoming.send(()).ok();
        }

        // stop ourselves :)
        self.transport_outgoing_tx.take();
    }

    pub async fn send_msg<L: LoginCredentials>(
        &mut self,
        msg: IRCMessage,
    ) -> Result<(), CErr<T, L>> {
        if let Ok(Some(_)) | Err(_) = self.rx_kill_outgoing.try_recv() {
            // ConnectionIncoming part has either failed (Ok(Some(_)))
            // or ended (end of stream) (Err(_)).
            //
            // Stop sending more messages.
            // Drop this half so the TCP stream gets closed when both halves are dropped
            drop(self.transport_outgoing_tx.take());
            return Err(CErr::<T, L>::Closed());
        }

        // this error condition can additionally trigger if the connection was closed using .close()
        // on the ConnectionOutgoing
        let outgoing_messages = self
            .transport_outgoing_tx
            .as_mut()
            .ok_or_else(|| CErr::<T, L>::Closed())?;

        log::trace!("> {}", msg.as_raw_irc());

        let send_result = outgoing_messages
            .send(msg)
            .await
            .map_err(CErr::<T, L>::OutgoingError);

        if send_result.is_err() {
            self.stop_incoming_and_outgoing();
        }

        send_result
    }

    pub async fn join<L: LoginCredentials>(&mut self, channel: String) -> Result<(), CErr<T, L>> {
        self.send_msg::<L>(irc!["JOIN", format!("#{}", channel)])
            .await?;
        // on success add channel
        self.channels.insert(channel);

        Ok(())
    }

    // TODO this is not right. On error condition, we need to detect whether the
    //  TwitchIRCClient has already "read" the channels to re-join before.
    //  If yes -> Some kind of special error, to let the upper loop know to retry
    //  If no  -> Fine, just bubble up the error but the channel should be removed from the set
    pub async fn part<L: LoginCredentials>(&mut self, channel: &str) -> Result<(), CErr<T, L>> {
        let res = self
            .send_msg::<L>(irc!["PART", format!("#{}", channel)])
            .await;
        // remove channel regardless of success
        self.channels.remove(channel);

        res
    }
    // TODO figure out how to do a part() correctly

    pub async fn close(&mut self) {
        self.stop_incoming_and_outgoing();
    }
}
