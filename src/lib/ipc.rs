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
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use crate::{ZatelConnection, ZatelError, ZatelLogEntry, ZatelPluginInfo};

const DEFAULT_SOCKET_PATH: &str = "/tmp/zatel_socket";
const IPC_SAFE_SIZE: usize = 1024 * 1024 * 10; // 10 MiB

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub enum ZatelIpcData {
    Error(ZatelError),
    QueryPluginInfo,
    QueryPluginInfoReply(ZatelPluginInfo),
    QueryIfaceInfo(String),
    QueryIfaceInfoReply(String),
    // Plugin with ZatelPluginCapacity::Apply capacity should support
    // ValidateConf and only reply with the supported portion of desire config.
    ValidateConf(String),
    ValidateConfReply(String),
    // Plugin with ZatelPluginCapacity::Config capacity should support
    // SaveConf and reply with the UUID saved.
    SaveConf(ZatelConnection),
    SaveConfReply(ZatelConnection),
    QuerySavedConf(String),
    QuerySavedConfReply(ZatelConnection),
    QuerySavedConfAll,
    QuerySavedConfAllReply(Vec<ZatelConnection>),
    ConnectionClosed,
    None,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ZatelIpcMessage {
    pub data: ZatelIpcData,
    // TODO: include logs also
    pub log: Option<Vec<ZatelLogEntry>>,
}

impl ZatelIpcMessage {
    pub fn new(data: ZatelIpcData) -> Self {
        ZatelIpcMessage {
            data: data,
            log: None,
        }
    }
    pub fn new_with_log(data: ZatelIpcData, log: Vec<ZatelLogEntry>) -> Self {
        ZatelIpcMessage {
            data: data,
            log: Some(log),
        }
    }

    pub fn from_result(result: Result<Self, ZatelError>) -> Self {
        match result {
            Ok(i) => i,
            Err(e) => ZatelIpcMessage::new(ZatelIpcData::Error(e)),
        }
    }

    pub fn get_data_str<'a>(&'a self) -> Result<&'a str, ZatelError> {
        match &self.data {
            ZatelIpcData::QueryIfaceInfo(s) => Ok(&s),
            ZatelIpcData::QueryIfaceInfoReply(s) => Ok(&s),
            ZatelIpcData::ValidateConf(s) => Ok(&s),
            ZatelIpcData::ValidateConfReply(s) => Ok(&s),
            _ => Err(ZatelError::invalid_argument(format!(
                "{:?} does not holding string in data",
                &self.data
            ))),
        }
    }
}

pub fn ipc_bind() -> Result<UnixListener, ZatelError> {
    ipc_bind_with_path(DEFAULT_SOCKET_PATH)
}

pub fn ipc_bind_with_path(
    socket_path: &str,
) -> Result<UnixListener, ZatelError> {
    remove_file(socket_path).ok();
    match UnixListener::bind(socket_path) {
        Err(e) => Err(ZatelError::bug(format!(
            "Failed to bind socket {}: {}",
            socket_path, e
        ))),
        Ok(l) => Ok(l),
    }
}

pub async fn ipc_connect() -> Result<UnixStream, ZatelError> {
    ipc_connect_with_path(DEFAULT_SOCKET_PATH).await
}

pub async fn ipc_connect_with_path(
    socket_path: &str,
) -> Result<UnixStream, ZatelError> {
    match UnixStream::connect(socket_path).await {
        Err(e) => Err(ZatelError::bug(format!(
            "Failed to connect socket {}: {}",
            socket_path, e
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

async fn ipc_recv_get_size(
    stream: &mut UnixStream,
) -> Result<usize, ZatelError> {
    match stream.read_u32().await {
        Err(e) => {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(0); // connection closed.
            } else {
                // TODO: Handle the client closed the connection.
                return Err(ZatelError::bug(format!(
                    "Failed to read message size: {:?}",
                    e
                )));
            }
        }
        Ok(s) => Ok(s as usize),
    }
}

async fn ipc_recv_get_data(
    stream: &mut UnixStream,
    message_size: usize,
) -> Result<ZatelIpcMessage, ZatelError> {
    let mut buffer = vec![0u8; message_size];

    if let Err(e) = stream.read_exact(&mut buffer).await {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            return Ok(ZatelIpcMessage::new(ZatelIpcData::ConnectionClosed));
        } else {
            return Err(ZatelError::bug(format!(
                "Failed to read message to buffer with size {}: {}",
                message_size, e
            )));
        }
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

pub async fn ipc_recv(
    stream: &mut UnixStream,
) -> Result<ZatelIpcMessage, ZatelError> {
    let message_size = ipc_recv_get_size(stream).await?;
    if message_size == 0 {
        return Ok(ZatelIpcMessage::new(ZatelIpcData::ConnectionClosed));
    }
    ipc_recv_get_data(stream, message_size).await
}

// Return error if data size execeed IPC_SAFE_SIZE
// Normally used by daemon where client can not be trusted.
pub async fn ipc_recv_safe(
    stream: &mut UnixStream,
) -> Result<ZatelIpcMessage, ZatelError> {
    let message_size = ipc_recv_get_size(stream).await?;
    if message_size == 0 {
        return Ok(ZatelIpcMessage::new(ZatelIpcData::ConnectionClosed));
    }
    if message_size > IPC_SAFE_SIZE {
        return Err(ZatelError::invalid_argument(format!(
            "Invalid IPC message: message size execeed the limit({})",
            IPC_SAFE_SIZE
        )));
    }
    ipc_recv_get_data(stream, message_size).await
}

pub async fn ipc_exec(
    stream: &mut UnixStream,
    message: &ZatelIpcMessage,
) -> Result<ZatelIpcMessage, ZatelError> {
    ipc_send(stream, message).await?;
    ipc_recv(stream).await
}
