use anyhow::ensure;
use anyhow::Context;
use messages::server::ServerCommand;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::ToSocketAddrs;

pub mod messages;

pub struct MultiTx(OwnedWriteHalf);
pub struct MultiRx(OwnedReadHalf);

pub async fn connect(addr: impl ToSocketAddrs) -> anyhow::Result<(MultiRx, MultiTx)> {
    let (rx, tx) = tokio::net::TcpStream::connect(addr).await?.into_split();
    Ok((MultiRx(rx), MultiTx(tx)))
}

impl MultiRx {
    pub async fn read(&mut self) -> anyhow::Result<messages::client::ClientCommand> {
        let msg_type = self.0.read_u8().await.context("Read msg type")?;

        ensure!(msg_type == 1, "Not JSON_LINE");
        let mut json_str = vec![];

        loop {
            let b = self.0.read_u8().await.context("Read msg char")?;
            if b == b'\n' {
                break;
            }
            json_str.push(b);
        }

        Ok(serde_json::from_slice(&json_str)?)
    }
}

impl MultiTx {
    pub async fn write(&mut self, cmd: &messages::server::ServerCommand) -> anyhow::Result<()> {
        let data = match cmd {
            ServerCommand::Raw(x) => Vec::from(x.trim().as_bytes()),
            cmd => serde_json::to_vec(cmd)?,
        };

        self.0.write_u8(1).await?;
        self.0.write_all(&data).await?;
        self.0.write_u8(b'\n').await?;
        self.0.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::messages::{client::Joined, get_topic, server::Auth, types::Room};

    use super::*;

    #[test]
    fn ser() {
        let msg = messages::server::ServerCommand::Auth(Auth {
            password: "".into(),
            name: "Drewol".into(),
            version: "test".into(),
        });

        let val = serde_json::to_value(&msg).expect("Failed to convert to json value");
        let topic = val.get("topic").expect("no topic");
        let topic = topic.as_str().expect("null topic");
        assert_eq!(topic, "user.auth");
    }

    #[test]
    fn de() {
        let msg = json!({
            "topic": "server.room.joined",
            "room": {
                "id": "test",
                "name": "test room",
                "current": 1,
                "max": 10,
                "ingame": true,
                "password": false,
            }
        });

        let value: messages::client::ClientCommand =
            serde_json::from_value(msg).expect("Failed to deserialize");
        assert!(matches!(
            value,
            messages::client::ClientCommand::Joined(Joined {
                room: Room {
                    id,
                    current,
                    max,
                    name,
                    ingame,
                    password,
                    join_token
                }
            }) if id == "test"
            && current == 1
            && max == 10
            && ingame
            && !password
            && join_token.is_none()
            && name == "test room"
        ))
    }

    #[test]
    fn topic_extractor() {
        let topic = get_topic(messages::client::ClientCommand::BadPassword {})
            .expect("Could not extract topic");

        assert_eq!(topic, "server.room.badpassword");

        let topic = get_topic(messages::client::ClientCommand::Started(
            messages::client::Started {
                hard: false,
                mirror: false,
            },
        ))
        .expect("Could not extract topic");

        assert_eq!(topic, "game.started");

        let topic = get_topic(messages::server::ServerCommand::ToggleHostRotate)
            .expect("Could not extract topic");

        assert_eq!(topic, "room.option.rotation.toggle");
    }
}
