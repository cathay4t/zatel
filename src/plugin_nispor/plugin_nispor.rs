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

mod iface;

use std::env::args;

use nispor::NetState;
use serde_yaml;
use tokio::{self, io::AsyncWriteExt, net::UnixStream};
use zatel::{
    ipc_bind_with_path, ipc_recv, ipc_send, ZatelError, ZatelIpcData,
    ZatelIpcMessage, ZatelPluginCapacity, ZatelPluginInfo,
};

use crate::iface::ZatelBaseIface;

const PLUGIN_NAME: &str = "nispor";

#[tokio::main()]
async fn main() {
    let argv: Vec<String> = args().collect();

    if argv.len() != 2 {
        eprintln!(
            "Invalid argument, should be single argument: <plugin_socket_path>"
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
                            "{}: failed to send to daemon : {}",
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

// TODO: The lib zatel should provide function call `plugin_start` taking
//       below function pointer as argument. But it is complex to passing
//       async function to a thread.
async fn handle_msg(data: ZatelIpcData) -> ZatelIpcMessage {
    eprintln!("DEBUG: {}: Got request: {:?}", PLUGIN_NAME, data);
    match data {
        ZatelIpcData::QueryIfaceInfo(iface_name) => {
            ZatelIpcMessage::from_result(query_iface(&iface_name))
        }
        ZatelIpcData::QueryPluginInfo => ZatelIpcMessage::new(
            ZatelIpcData::QueryPluginInfoReply(ZatelPluginInfo::new(
                PLUGIN_NAME,
                vec![ZatelPluginCapacity::Query, ZatelPluginCapacity::Apply],
            )),
        ),
        ZatelIpcData::ValidateConf(conf) => {
            ZatelIpcMessage::from_result(validate_conf(&conf))
        }
        _ => {
            eprintln!(
                "WARN: {}: Got unknown request: {:?}",
                PLUGIN_NAME, &data
            );
            ZatelIpcMessage::new(ZatelIpcData::None)
        }
    }
}

fn query_iface(iface_name: &str) -> Result<ZatelIpcMessage, ZatelError> {
    let net_state = match NetState::retrieve() {
        Ok(s) => s,
        Err(e) => {
            return Err(ZatelError::plugin_error(format!(
                "nispor::NetState::retrieve() failed with {}",
                e
            )))
        }
    };
    match net_state.ifaces.get(iface_name) {
        Some(iface_info) => {
            let zatel_iface: ZatelBaseIface = iface_info.into();
            match serde_yaml::to_string(&zatel_iface) {
                Ok(s) => Ok(ZatelIpcMessage::new(
                    ZatelIpcData::QueryIfaceInfoReply(s),
                )),
                Err(e) => Err(ZatelError::plugin_error(format!(
                    "Failed to convert ZatelIfaceInfo to yml: {}",
                    e
                ))),
            }
        }
        None => Err(ZatelError::invalid_argument(format!(
            "Interface {} not found",
            iface_name
        ))),
    }
}

fn validate_conf(conf: &str) -> Result<ZatelIpcMessage, ZatelError> {
    if let Ok(zatel_iface) = serde_yaml::from_str::<ZatelBaseIface>(conf) {
        if let Ok(s) = serde_yaml::to_string(&zatel_iface) {
            return Ok(ZatelIpcMessage::new(ZatelIpcData::ValidateConfReply(
                s,
            )));
        }
    }
    Ok(ZatelIpcMessage::new(ZatelIpcData::ValidateConfReply(
        "".into(),
    )))
}
