use super::login::LoginCredentials;
use super::message::AsRawIRC;
use super::message::IRCMessage;
use tokio::io::Result as TokioIoResult;
use tokio::net::TcpStream;
use tokio::prelude::*;

//#[async_trait]
//trait Transport {
//    async fn send_raw(&mut self, line: &str) -> TokioIoResult<()>;
//
//    async fn send<T: AsRawIRC + Send>(&mut self, message: T) -> TokioIoResult<()> {
//        self.send_raw(&message.as_raw_irc()).await
//    }
//}

pub struct Connection {
    socket: TcpStream,
    pub(crate) channels: Vec<String>,
}

impl Connection {
    pub async fn new(login: &LoginCredentials) -> TokioIoResult<Connection> {
        let socket = TcpStream::connect(("irc.chat.twitch.tv", 6667)).await?;
        let mut connection = Connection {
            socket,
            channels: vec![],
        };
        connection
            .send(IRCMessage::new_simple(
                "CAP",
                vec!["REQ", "twitch.tv/tags twitch.tv/commands"],
            ))
            .await?;
        login.execute(&mut connection).await?;
        Ok(connection)
    }

    pub async fn send_raw(&mut self, line: &str) -> TokioIoResult<()> {
        if line.contains('\r') || line.contains('\n') {
            panic!("invalid input: given line must not contain newlines")
        }

        self.socket
            .write_all(format!("{}\r\n", line).as_bytes())
            .await?;
        Ok(())
    }

    pub async fn send<T: AsRawIRC>(&mut self, message: T) -> TokioIoResult<()> {
        self.send_raw(&message.as_raw_irc()).await
    }
}
