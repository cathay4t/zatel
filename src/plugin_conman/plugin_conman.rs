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

use std::env::args;

use serde::{Deserialize, Serialize};
use tokio::{self, io::AsyncWriteExt, net::UnixStream};
use zatel::{
    ipc_bind_with_path, ipc_recv, ipc_send, ZatelError, ZatelIpcData,
    ZatelIpcMessage, ZatelPluginCapacity, ZatelPluginInfo,
};

const PLUGIN_NAME: &str = "foo";
const CONF_FOLDER: &str = "/tmp/zatel";

#[tokio::main()]
async fn main() {
    let argv: Vec<String> = args().collect();

    if argv.len() != 2 {
        eprintln!(
            "Invalid argument, should be single argument: <plugin_socket_path>"
        );
        std::process::exit(1);
    }

    if let Err(e) = create_conf_dir() {
        eprintln!(
            "Failed to create folder for saving configurations {}: {}",
            CONF_FOLDER, e
        );
        std::process::exit(1);
    }

    let socket_path = &argv[1];

    let listener = match ipc_bind_with_path(socket_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            return;
        }
    };
    eprintln!("DEBUG: {}: listening on {}", PLUGIN_NAME, socket_path);

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                // TODO: Limit the maximum connected client as it could
                //       from suspicious source, not daemon
                tokio::task::spawn(async move { handle_client(stream).await });
            }
            Err(e) => {
                eprintln!("{}", e);
            }
        }
    }
}

async fn shutdown_connection(stream: &mut UnixStream) {
    if let Err(e) = stream.shutdown().await {
        eprintln!("{}", e);
    }
}

// TODO: Implement on:
//  * timeout
async fn handle_client(mut stream: UnixStream) {
    loop {
        match ipc_recv(&mut stream).await {
            Ok(ipc_msg) => match ipc_msg.data {
                ZatelIpcData::ConnectionClosed => {
                    shutdown_connection(&mut stream).await;
                    break;
                }
                _ => {
                    let message = handle_msg(ipc_msg.data).await;
                    eprintln!("DEBUG: {}: reply: {:?}", PLUGIN_NAME, &message);
                    if let Err(e) = ipc_send(&mut stream, &message).await {
                        eprintln!(
                            "DEBUG: {}: failed to send to daemon : {}",
                            PLUGIN_NAME, e
                        );
                    }
                }
            },
            Err(e) => {
                eprintln!("IPC error {}", e);
                shutdown_connection(&mut stream).await;
                break;
            }
        }
    }
}

async fn handle_msg(data: ZatelIpcData) -> ZatelIpcMessage {
    eprintln!("DEBUG: {}: Got request: {:?}", PLUGIN_NAME, data);
    match data {
        ZatelIpcData::QueryPluginInfo => ZatelIpcMessage::new(
            ZatelIpcData::QueryPluginInfoReply(ZatelPluginInfo::new(
                PLUGIN_NAME,
                vec![ZatelPluginCapacity::Config],
            )),
        ),
        ZatelIpcData::SaveConf(uuid, conf) => {
            ZatelIpcMessage::from_result(save_conf(&uuid, &conf))
        }
        _ => ZatelIpcMessage::new(ZatelIpcData::None),
    }
}

fn save_conf(uuid: &str, conf: &str) -> Result<ZatelIpcMessage, ZatelError> {
    Ok(ZatelIpcMessage::new(ZatelIpcData::SaveConfReply(
        uuid.to_string(),
    )))
}

fn create_conf_dir() -> Result<(), ZatelError> {
    if !std::path::Path::new(CONF_FOLDER).is_dir() {
        std::fs::remove_file(CONF_FOLDER).ok();
        if let Err(e) = std::fs::create_dir(CONF_FOLDER) {
            return Err(ZatelError::plugin_error(format!(
                "Failed to create folder {}: {}",
                CONF_FOLDER, e
            )));
        }
    }
    Ok(())
}
