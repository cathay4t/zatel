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

use serde_yaml;
use tokio::{self, io::AsyncWriteExt, net::UnixStream, task};
use uuid::Uuid;
use zatel::{
    ipc_bind, ipc_plugins_exec, ipc_recv_safe, ipc_send, ZatelError,
    ZatelIpcData, ZatelIpcMessage, ZatelPluginCapacity, ZatelPluginInfo,
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
    let plugins = load_plugins().await;

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
                ZatelIpcData::SaveConf(uuid, conf) => {
                    handle_save_conf(&mut stream, &uuid, &conf, plugins).await;
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

    let reply_msg = match merge_yaml_mapping_strs(
        &ipc_plugins_exec(&ipc_msg, plugins, &ZatelPluginCapacity::Query).await,
    ) {
        Ok(s) => ZatelIpcMessage::new(ZatelIpcData::QueryIfaceInfoReply(s)),
        Err(e) => {
            if let Err(e) =
                ipc_send(stream, &ZatelIpcMessage::new(ZatelIpcData::Error(e)))
                    .await
            {
                eprintln!("ERROR: {}", e);
            }
            return;
        }
    };

    if let Err(e) = ipc_send(stream, &reply_msg).await {
        eprintln!("ERROR: Failed to send IPC message: {}", e);
    }
}

// Steps:
//  1. Send conf string to plugin to validate. Raise error if existing plugins
//     cannot achieve full desire config.
//  2. Send conf string to plugin to save.
//
async fn handle_save_conf(
    stream: &mut UnixStream,
    uuid: &str,
    conf: &str,
    plugins: &[ZatelPluginInfo],
) {
    eprintln!("DEBUG: handle_save_conf {}", conf);

    if let Err(e) = validate_conf(conf, plugins).await {
        if let Err(e) =
            ipc_send(stream, &ZatelIpcMessage::new(ZatelIpcData::Error(e)))
                .await
        {
            eprintln!("WARN: {}", e);
        }
        return;
    }

    // Gen UUID for this config
    let uuid = if uuid == "" {
        format!(
            "{}",
            Uuid::new_v4()
                .to_hyphenated()
                .encode_lower(&mut Uuid::encode_buffer())
        )
    } else {
        uuid.to_string()
    };

    let ipc_msg = ZatelIpcMessage::new(ZatelIpcData::SaveConf(
        uuid.clone(),
        conf.to_string(),
    ));

    let reply_data =
        if ipc_plugins_exec(&ipc_msg, plugins, &ZatelPluginCapacity::Config)
            .await
            .len()
            == 0
        {
            ZatelIpcData::Error(ZatelError::plugin_error(format!(
                "No plugin has saved desired config"
            )))
        } else {
            ZatelIpcData::SaveConfReply(uuid)
        };

    if let Err(e) = ipc_send(stream, &ZatelIpcMessage::new(reply_data)).await {
        eprintln!("ERROR: Failed to send IPC message: {}", e);
    }
}

// Each plugin could only cover a portion of the configure, but they should
// sum up to the full desire config, or else return ZatelError
async fn validate_conf(
    conf: &str,
    plugins: &[ZatelPluginInfo],
) -> Result<(), ZatelError> {
    eprintln!("DEBUG: validate_conf {}", conf);
    let ipc_msg =
        ZatelIpcMessage::new(ZatelIpcData::ValidateConf(conf.to_string()));

    let desire_yaml_mapping: serde_yaml::Value =
        match serde_yaml::from_str(conf) {
            Ok(i) => i,
            Err(e) => {
                return Err(ZatelError::invalid_argument(format!(
                    "Invalid format of YAML: {}",
                    e
                )));
            }
        };

    let reply_strs =
        ipc_plugins_exec(&ipc_msg, plugins, &ZatelPluginCapacity::Apply).await;
    let merged_reply = merge_yaml_mapping_strs(&reply_strs)?;
    let validated_yaml_mapping: serde_yaml::Value =
        match serde_yaml::from_str(&merged_reply) {
            Ok(i) => i,
            Err(e) => {
                return Err(ZatelError::bug(format!(
                    "This should never happen: {}",
                    e
                )));
            }
        };

    if validated_yaml_mapping != desire_yaml_mapping {
        // TODO: provide fancy difference to user via error.
        Err(ZatelError::invalid_argument(format!(
            "Invalid config, validated: {}, desired: {}",
            &merged_reply, conf
        )))
    } else {
        Ok(())
    }
}

fn merge_yaml_mapping_strs(yml_strs: &[String]) -> Result<String, ZatelError> {
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
