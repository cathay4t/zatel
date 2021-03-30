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

mod plugin;

use futures::future::join_all;

use serde_yaml;
use tokio::{self, io::AsyncWriteExt, net::UnixStream, task};
use zatel::{
    ipc_bind, ipc_plugin_exec, ipc_recv_safe, ipc_send, ZatelError,
    ZatelIpcData, ZatelIpcMessage, ZatelPluginInfo,
};

use crate::plugin::load_plugins;

#[tokio::main(flavor = "multi_thread", worker_threads = 50)]
async fn main() {
    let listener = match ipc_bind() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    // We don't plan to unload plugin during runtime when plugin is slow or bad.
    // To support that, we need a mutex protected Vec which is complex.
    // We assume the plugin is trustable.
    let plugins = load_plugins();

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                eprintln!("DEBUG: daemon: IPC client connected");
                // TODO: Limit the maximum connected client.
                let plugins_clone = plugins.clone();
                task::spawn(async move {
                    handle_client(stream, &plugins_clone).await
                });
            }
            Err(e) => {
                eprintln!("{}", e);
            }
        }
    }
}

async fn shutdown_connection(stream: &mut UnixStream) {
    if let Err(e) = stream.shutdown().await {
        eprintln!("ERROR: Daemon: failed to shutdown a connection: {}", e);
    }
}

// TODO: Implement on:
//  * timeout
async fn handle_client(mut stream: UnixStream, plugins: &[ZatelPluginInfo]) {
    loop {
        match ipc_recv_safe(&mut stream).await {
            Ok(ipc_msg) => match ipc_msg.data {
                ZatelIpcData::ConnectionClosed => {
                    shutdown_connection(&mut stream).await;
                    break;
                }
                ZatelIpcData::QueryIfaceInfo(filter) => {
                    handle_query(&mut stream, &filter, plugins).await;
                }
                _ => {
                    println!("got unknown IPC message: {:?}", &ipc_msg);
                    if let Err(e) = ipc_send(
                        &mut stream,
                        &ZatelIpcMessage::new(ZatelIpcData::Error(
                            ZatelError::invalid_argument(format!(
                                "Invalid IPC message: {:?}",
                                &ipc_msg
                            )),
                        )),
                    )
                    .await
                    {
                        eprintln!("Got failure when reply to client: {}", e);
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

async fn handle_query(
    stream: &mut UnixStream,
    filter: &str,
    plugins: &[ZatelPluginInfo],
) {
    eprintln!("DEBUG: handle_query {}", filter);
    let ipc_msg =
        ZatelIpcMessage::new(ZatelIpcData::QueryIfaceInfo(filter.into()));
    let mut replys_async = Vec::new();
    for plugin_info in plugins {
        replys_async.push(ipc_plugin_exec(plugin_info, &ipc_msg));
    }
    let replys = join_all(replys_async).await;

    let mut iface_strs = Vec::new();
    for reply in replys {
        eprintln!("DEBUG: Got reply from plugin: {:?}", &reply);
        if let Ok(ZatelIpcMessage {
            data: ZatelIpcData::QueryIfaceInfoReply(s),
            log: _,
        }) = reply
        {
            iface_strs.push(s);
        }
    }
    if let Err(e) = match merge_iface_strs(&iface_strs) {
        Ok(s) => {
            ipc_send(
                stream,
                &ZatelIpcMessage::new(ZatelIpcData::QueryIfaceInfoReply(s)),
            )
            .await
        }
        Err(e) => {
            ipc_send(
                stream,
                &ZatelIpcMessage::new(ZatelIpcData::Error(
                    ZatelError::plugin_error(format!("{}", e)),
                )),
            )
            .await
        }
    } {
        eprintln!("ERROR: Failed to send IPC message: {}", e);
    }
}

fn merge_iface_strs(yml_strs: &[String]) -> Result<String, ZatelError> {
    let mut full_obj = serde_yaml::Mapping::new();

    for yml_str in yml_strs {
        let cur_obj: serde_yaml::Value = match serde_yaml::from_str(yml_str) {
            Ok(i) => i,
            Err(e) => {
                return Err(ZatelError::plugin_error(format!(
                    "Invalid format of YAML reply from plugin: {}, {}",
                    yml_str, e
                )))
            }
        };
        match cur_obj.as_mapping() {
            Some(cur) => {
                for (key, value) in cur.iter() {
                    if full_obj.contains_key(key) {
                        let old_value = &full_obj[key];
                        if old_value != value {
                            eprintln!(
                                "WARN: duplicate key: {:?} {:?} vs {:?}",
                                key, value, old_value
                            );
                        }
                    } else {
                        full_obj.insert(key.clone(), value.clone());
                    }
                }
            }
            None => {
                eprintln!("WARN: {:?} is not mapping", cur_obj);
            }
        }
    }

    match serde_yaml::to_string(&full_obj) {
        Ok(s) => Ok(s),
        Err(e) => Err(ZatelError::bug(format!(
            "Failed to convert serde_yaml::Mapping to string: {:?} {}",
            &full_obj, e
        ))),
    }
}
