pub mod error;
pub mod main_loop;

use crate::config::{ClientConfig, LoginCredentials};
use crate::connection::error::ConnErr;
use crate::connection::main_loop::{MainLoopCommand, MainLoopWorker};
use crate::message::commands::ServerMessage;
use crate::message::IRCMessage;
use crate::transport::Transport;
use futures::channel::{mpsc, oneshot};
use std::sync::Arc;

pub struct Connection<T: Transport<L>, L: LoginCredentials> {
    /// sends commands to the this connection's main event loop.
    main_loop_tx: mpsc::UnboundedSender<MainLoopCommand<T, L>>,
    /// provides the incoming messages. This is an `Option<>` so it can be taken ownership of using
    /// `.take()`
    pub connection_incoming_rx:
        Option<mpsc::UnboundedReceiver<Result<ServerMessage, ConnErr<T, L>>>>,
}

impl<T: Transport<L>, L: LoginCredentials> Connection<T, L> {
    pub fn new(config: Arc<ClientConfig<L>>) -> Connection<T, L> {
        let (main_loop_tx, main_loop_rx) = mpsc::unbounded();
        let (connection_incoming_tx, connection_incoming_rx) = mpsc::unbounded();

        MainLoopWorker::new(
            config,
            connection_incoming_tx,
            main_loop_tx.clone(),
            main_loop_rx,
        )
        .spawn();

        Connection {
            main_loop_tx,
            connection_incoming_rx: Some(connection_incoming_rx),
        }
    }

    pub async fn send_message(&mut self, message: IRCMessage) -> Result<(), ConnErr<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        // unwrap: We don't expect the main loop to exit (and drop the Receiver) as long
        // as this Connection handle lives
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::SendMessage(message, Some(return_tx)))
            .unwrap();
        // unwrap: The main loop will always reply instead of dropping the sender.
        return return_rx.await.unwrap();
    }

    pub async fn join(&mut self, channel_login: String) -> Result<(), ConnErr<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::Join(channel_login, return_tx))
            .unwrap();
        return return_rx.await.unwrap();
    }

    pub async fn part(&mut self, channel_login: String) -> Result<(), ConnErr<T, L>> {
        let (return_tx, return_rx) = oneshot::channel();
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::Part(channel_login, return_tx))
            .unwrap();
        return return_rx.await.unwrap();
    }

    pub async fn close(&mut self) {
        let (return_tx, return_rx) = oneshot::channel();
        // params for MainLoopCommand::Clone:
        // 1) optional reason error, 2) return channel
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::Close(None, Some(return_tx)))
            .unwrap();
        return return_rx.await.unwrap();
    }
}

impl<T: Transport<L>, L: LoginCredentials> Drop for Connection<T, L> {
    fn drop(&mut self) {
        // send the main Loop a Close command, so it ends itself as soon as every clone
        // of main_loop_tx is dropped

        // params for MainLoopCommand::Clone:
        // 1) optional reason error, 2) return channel
        self.main_loop_tx
            .unbounded_send(MainLoopCommand::Close(None, None))
            .unwrap();
    }
}

// pub struct Connection<T: Transport, L: LoginCredentials> {
//     state: Arc<Mutex<ConnectionState<T, L>>>,
//     outgoing: Arc<Mutex<Option<ConnectionOutgoing<T>>>>,
//     incoming: Option<mpsc::Receiver<Result<ServerMessage, ConnErr<T, L>>>>,
// }
//
// enum ConnectionState<T: Transport, L: LoginCredentials> {
//     Open {
//         channels: HashSet<String>,
//         tx_kill_incoming: oneshot::Sender<()>,
//     },
//     Closed,
// }
//
// pub struct ConnectionOutgoing<T: Transport> {
//     transport_outgoing_tx: Option<T::Outgoing>
// }
//
// pub struct ConnectionIncomingWorker<T: Transport, L: LoginCredentials> {
//     /// common state for incoming and outgoing
//     state: Arc<Mutex<Connection<T, L>>>,
//     /// incoming stream of Result<IRCMessage, ConnErr<T, L>> from the transport
//     transport_incoming_rx: Option<T::Incoming>,
//     /// mpsc channel to send processed messages further down, to the Client
//     connection_incoming_tx: mpsc::Sender<Result<ServerMessage, ConnErr<T, L>>>,
//     /// used to stop the worker if there is an error during sending.
//     rx_kill_incoming: future::Fuse<oneshot::Receiver<()>>,
// }
//
// impl<T: Transport, L: LoginCredentials> Connection<T, L> {
//     pub fn new(config: Arc<ClientConfig<L>>) -> Connection<T, L> {
//         let (tx_kill_incoming, rx_kill_incoming) = oneshot::channel();
//         let (connection_incoming_tx, connection_incoming_rx) = mpsc::channel(16);
//
//         let state = Arc::new(Mutex::new(ConnectionState::Open {
//             channels: HashSet::new(),
//             tx_kill_incoming,
//         }));
//         let outgoing = Arc::new(Mutex::new(None));
//         let incoming = Some(connection_incoming_rx);
//
//         let incoming_worker = ConnectionIncomingWorker {
//             state: state.clone(),
//             transport_incoming_rx: None,
//             connection_incoming_tx,
//             rx_kill_incoming: rx_kill_incoming.fuse()
//         };
//
//         tokio::spawn(Connection::init_task(incoming_worker, outgoing.clone(), config));
//
//         Connection {
//             state,
//             incoming,
//             outgoing,
//         }
//     }
//
//     fn init_task(
//         mut incoming: ConnectionIncoming<T, L>,
//         outgoing: Arc<Mutex<Connection<T>>>,
//         config: Arc<ClientConfig<L>>,
//     ) -> impl Future<Output = ()> + 'static {
//         // pack the Mutex and its guard together so the packed-together values can be moved to the
//         // async block without getting problems from the borrow checker
//         let mut outgoing_locked = MutexAndGuard::new(outgoing, |outgoing| {
//             outgoing
//                 .try_lock()
//                 .expect("init_task called while outgoing is locked")
//         });
//
//         async move {
//             // we need both the "get login token" and "connect transport" to succeed
//             // so this is grouped to handle both errors at once
//             let maybe_transport_and_login_token: Result<(T, Option<String>), ConnErr<T, L>> =
//                 async {
//                     let token = config
//                         .login_credentials
//                         .get_token()
//                         .await
//                         .map_err(ConnectionError::LoginError)?
//                         .clone();
//                     let transport = T::new().await.map_err(ConnectionError::ConnectError)?;
//                     Ok((transport, token))
//                 }
//                     .await;
//
//             // if either there is no login token or the connection failed to connect then we have to abort
//             let (transport, token) = match maybe_transport_and_login_token {
//                 Err(init_error) => {
//                     // closes the contained channel too, so a downstream reading
//                     // from the incoming_messages will end
//                     incoming.on_init_error(init_error).await;
//
//                     // exit the init task
//                     return;
//                 }
//                 Ok(e) => e,
//             };
//
//             let (transport_incoming_rx, transport_outgoing_tx) = transport.split();
//
//             incoming.transport_incoming_rx = Some(transport_incoming_rx);
//             outgoing_locked.transport_outgoing_tx = Some(transport_outgoing_tx);
//
//             let login = config.login_credentials.get_login().to_owned();
//
//             // Start ConnectionIncoming task
//             tokio::spawn(incoming.start());
//
//             // initialize the IRC connection with setup commands
//             outgoing_locked
//                 .send_msg::<L>(irc!["CAP", "REQ", "twitch.tv/commands twitch.tv/tags"])
//                 .await
//                 .ok();
//             if let Some(token) = token {
//                 outgoing_locked
//                     .send_msg::<L>(irc!["PASS", format!("oauth:{}", token)])
//                     .await
//                     .ok();
//             }
//             outgoing_locked
//                 .send_msg::<L>(irc!["NICK", login])
//                 .await
//                 .ok();
//
//             // unlock the mutex, now allow messages to come in from the outside
//             drop(outgoing_locked);
//         }
//     }
// }
