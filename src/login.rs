use super::connection::Connection;
use crate::message::IRCMessage;
use tokio::io::Result as TokioIoResult;

pub struct LoginCredentials {
    pub nick: String,
    pub password: Option<String>,
}

impl LoginCredentials {
    pub async fn execute(&self, conn: &mut Connection) -> TokioIoResult<()> {
        if let Some(password) = &self.password {
            conn.send(IRCMessage::new_simple("PASS", vec![password]))
                .await?;
        }
        conn.send(IRCMessage::new_simple("NICK", vec![&self.nick]))
            .await?;
        Ok(())
    }
}
