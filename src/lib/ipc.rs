//    Copyright 2021 Red Hat, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fs::remove_file;

use serde::{Deserialize, Serialize};
use serde_yaml;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};

use crate::ZatelError;

const DEFAULT_SOCKET_PATH: &str = "/tmp/zatel_socket";

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum ZatelIpcCmd {
    Query(String),
    ConnectionClosed,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub enum ZatelIpcData {
    Error(ZatelError),
    String(String),
    None,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ZatelIpcMessage {
    pub cmd: ZatelIpcCmd,
    pub data: ZatelIpcData, // TODO: include logs also
}

pub fn ipc_bind() -> Result<UnixListener, ZatelError> {
    remove_file(DEFAULT_SOCKET_PATH).ok();
    match UnixListener::bind(DEFAULT_SOCKET_PATH) {
        Err(e) => Err(ZatelError::bug(format!(
            "Failed to bind socket {}: {}",
            DEFAULT_SOCKET_PATH, e
        ))),
        Ok(l) => Ok(l),
    }
}

pub async fn ipc_connect() -> Result<UnixStream, ZatelError> {
    match UnixStream::connect(DEFAULT_SOCKET_PATH).await {
        Err(e) => Err(ZatelError::bug(format!(
            "Failed to connect socket {}: {}",
            DEFAULT_SOCKET_PATH, e
        ))),
        Ok(l) => Ok(l),
    }
}

pub async fn ipc_send(
    stream: &mut UnixStream,
    message: &ZatelIpcMessage,
) -> Result<(), ZatelError> {
    let message_string = match serde_yaml::to_string(message) {
        Ok(s) => s,
        Err(e) => {
            return Err(ZatelError::invalid_argument(format!(
                "Invalid IPC message - failed to serialize {:?}: {}",
                &message, e
            )))
        }
    };
    let message_bytes = message_string.as_bytes();
    if let Err(e) = stream.write_u32(message_bytes.len() as u32).await {
        return Err(ZatelError::bug(format!(
            "Failed to write message size {} to socket: {}",
            message_bytes.len(),
            e
        )));
    };
    if let Err(e) = stream.write_all(message_bytes).await {
        return Err(ZatelError::bug(format!(
            "Failed to write message to socket: {}",
            e
        )));
    };
    Ok(())
}

pub async fn ipc_recv(
    stream: &mut UnixStream,
) -> Result<ZatelIpcMessage, ZatelError> {
    let message_size = match stream.read_u32().await {
        Err(e) => {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(ZatelIpcMessage {
                    cmd: ZatelIpcCmd::ConnectionClosed,
                    data: ZatelIpcData::None,
                });
            } else {
                // TODO: Handle the client closed the connection.
                return Err(ZatelError::bug(format!(
                    "Failed to read message size: {:?}",
                    e
                )));
            }
        }
        Ok(s) => s as usize,
    };
    let mut buffer = vec![0u8; message_size];

    if let Err(e) = stream.read_exact(&mut buffer).await {
        return Err(ZatelError::bug(format!(
            "Failed to read message to buffer with size {}: {}",
            message_size, e
        )));
    }
    match serde_yaml::from_slice::<ZatelIpcMessage>(&buffer) {
        Err(e) => Err(ZatelError::bug(format!(
            "Invalid message recieved: {:?}: {}",
            buffer, e
        ))),
        Ok(m) => match &m.data {
            ZatelIpcData::Error(e) => Err(e.clone()),
            _ => Ok(m),
        },
    }
}

pub async fn ipc_exec(
    stream: &mut UnixStream,
    message: &ZatelIpcMessage,
) -> Result<ZatelIpcMessage, ZatelError> {
    ipc_send(stream, message).await?;
    ipc_recv(stream).await
}
