// impl<T: Transport, L: LoginCredentials> ConnectionRx<T, L> {
//     fn stop_incoming_and_outgoing(&mut self) {
//         // stop tx
//         if let Some(tx_kill_outgoing) = self.tx_kill_outgoing.take() {
//             tx_kill_outgoing.send(()).ok();
//         }
//
//         // stop ourselves (end loop)
//         drop(self.transport_incoming_rx.take());
//     }
//
//     async fn on_message_from_transport(&mut self, message: IRCMessage) {
//         let raw_irc = message.as_raw_irc();
//         log::trace!("< {}", raw_irc);
//
//         let server_message = match ServerMessage::try_from(message) {
//             Ok(server_message) => server_message,
//             Err(err) => {
//                 // if message fails to parse, we ignore it
//                 // fixme error should be passed on ideally, just like the IRC parse errors
//                 log::warn!("Ignored incoming message that failed to parse as ServerMessage: `{}` (Error is: {:?})", raw_irc, err);
//                 return;
//             }
//         };
//
//         // TODO: ping/pong, RECONNECT, ... here
//
//         let send_err = self.connection_incoming_tx.send(Ok(server_message)).await;
//
//         if let Err(_) = send_err {
//             log::trace!("Rx task ending: receiving end dropped");
//             self.stop_incoming_and_outgoing();
//         }
//     }
//
//     async fn on_error_from_transport(&mut self, error: T::IncomingError) {
//         log::info!("Rx task ending: Error while reading from transport");
//
//         // send the error downstream
//         // .ok(): If an error occurs here, then the receiving end has been
//         // dropped, which is not a condition we need to handle here since
//         // we close this Connection anyways
//         self.connection_incoming_tx
//             .send(Err(ConnErr::<T, L>::IncomingError(error)))
//             .await
//             .ok();
//
//         // now stop the loop and the ConnectionOutgoing
//         self.stop_incoming_and_outgoing();
//     }
//
//     async fn on_init_error(&mut self, error: ConnErr<T, L>) {
//         // TODO update usages of Rx task/incoming task/etc in documentation, comments and log messages. and unify
//         log::info!("Rx task will not start (discarding): Initialization failure");
//
//         // .ok(): If an error occurs here, then the receiving end has been
//         // dropped, which is not a condition we need to handle here since
//         // we close this Connection anyways
//         self.connection_incoming_tx.send(Err(error)).await.ok();
//
//         self.stop_incoming_and_outgoing();
//     }
//
//     fn on_eof_from_transport(&mut self) {
//         log::info!("Rx task ending: EOF while reading from TCP socket");
//         self.stop_incoming_and_outgoing();
//     }
//
//     async fn start(mut self) {
//         // calling self.stop_incoming_and_outgoing() will cause this to break on
//         // the next iteration of the loop
//
//         while let Some(transport_incoming_rx) = &mut self.transport_incoming_rx {
//             // biased select: We want rx_kill_incoming to take priority.
//             futures::select_biased! {
//                 recv_result = (&mut self.rx_kill_incoming) => {
//                     // if result is Ok, then we definitely got command to shut down
//                     // if Err, then the sending part got dropped before sending something
//                     // (which is not supposed to happen)
//                     if recv_result.is_err() {
//                         log::warn!("ConnectionOutgoing was dropped before sending kill signal to ConnectionIncoming task")
//                     } else {
//                         // sender had an error, we need to shut down
//                         log::info!("ConnectionIncoming task ending: Received kill signal by ConnectionOutgoing");
//                     }
//                     self.stop_incoming_and_outgoing();
//                 },
//                 message = transport_incoming_rx.next() => {
//                     match message {
//                         Some(Ok(message)) => {
//                             // got a message
//                             self.on_message_from_transport(message).await;
//                         },
//                         Some(Err(error)) => {
//                             // stream encounters error
//                             self.on_error_from_transport(error).await;
//                         },
//                         None => {
//                             // stream ends without error
//                             self.on_eof_from_transport();
//                         }
//                     }
//                 },
//             }
//         }
//
//         log::info!("End of Rx task");
//         // TODO here: Pool needs to rejoin channels.
//         // also: modify ConnectionState if not already modified
//     }
// }
