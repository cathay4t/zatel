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
use serde_yaml;
use tokio::{self, io::AsyncWriteExt, net::UnixStream};
use zatel::{
    ipc_bind_with_path, ipc_connect, ipc_recv, ipc_send, ZatelError,
    ZatelIpcData, ZatelIpcMessage,
};


const PLUGIN_NAME: &str = "foo";

#[derive(Deserialize, Serialize, Debug, Clone)]
struct FooInfo {
    opt1: String,
    opt2: u32,
    opt3: Vec<u8>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
struct FooIface {
    name: String,
    foo: FooInfo,
}

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
                    let message = handle_msg(&mut stream, ipc_msg.data).await;
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

async fn handle_msg(
    stream: &mut UnixStream,
    data: ZatelIpcData,
) -> ZatelIpcMessage {
    eprintln!("DEBUG: {}: Got request: {:?}", PLUGIN_NAME, data);
    match data {
        ZatelIpcData::QueryIfaceInfo(iface_name) => {
            ZatelIpcMessage::from_result(query_iface(&iface_name))
        }
        _ => ZatelIpcMessage::new(ZatelIpcData::None),
    }
}

fn query_iface(iface_name: &str) -> Result<ZatelIpcMessage, ZatelError> {
    let iface = FooIface {
        name: iface_name.to_string(),
        foo: FooInfo {
            opt1: "opt1_value".into(),
            opt2: 8u32,
            opt3: vec![1, 2, 8],
        },
    };
    match serde_yaml::to_string(&iface) {
        Ok(s) => Ok(ZatelIpcMessage::new(ZatelIpcData::QueryIfaceInfoReply(s))),
        Err(e) => Err(ZatelError::plugin_error(format!(
            "Failed to convert ZatelIfaceInfo to yml: {}",
            e
        ))),
    }
}
