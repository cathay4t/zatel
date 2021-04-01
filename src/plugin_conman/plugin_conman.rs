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
use std::fs::OpenOptions;
use std::io::{Read, Write};

use serde_yaml;
use tokio::{self, io::AsyncWriteExt, net::UnixStream};
use zatel::{
    ipc_bind_with_path, ipc_recv, ipc_send, ZatelConnection, ZatelError,
    ZatelIpcData, ZatelIpcMessage, ZatelPluginCapacity, ZatelPluginInfo,
};

const PLUGIN_NAME: &str = "conman";
const CONF_FOLDER: &str = "/tmp/zatel";
const CONN_FILE_POSTFIX: &str = ".yml";

const CONNECTION_KEY: &str = "_connection";

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
        ZatelIpcData::SaveConf(ztl_con) => {
            ZatelIpcMessage::from_result(save_conf(ztl_con))
        }
        ZatelIpcData::QuerySavedConf(uuid) => {
            ZatelIpcMessage::from_result(query(&uuid))
        }
        ZatelIpcData::QuerySavedConfAll => {
            ZatelIpcMessage::from_result(query_all())
        }
        _ => ZatelIpcMessage::new(ZatelIpcData::None),
    }
}

fn save_conf(ztl_con: ZatelConnection) -> Result<ZatelIpcMessage, ZatelError> {
    let uuid = match &ztl_con.uuid {
        Some(u) => u,
        None => {
            return Err(ZatelError::bug(format!(
                "Got None uuid from daemon for connection {:?}",
                &ztl_con,
            )))
        }
    };
    let file_path = gen_file_path(uuid);
    let mut fd =
        match OpenOptions::new().create(true).write(true).open(&file_path) {
            Ok(f) => f,
            Err(e) => {
                return Err(ZatelError::plugin_error(format!(
                    "Failed to open file {}: {}",
                    &file_path, e
                )));
            }
        };

    let ztl_con_yaml = zatel_connection_to_flat_string(&ztl_con)?;

    if let Err(e) = fd.write_all(ztl_con_yaml.as_bytes()) {
        Err(ZatelError::plugin_error(format!(
            "Failed to write file {}: {}",
            &file_path, e
        )))
    } else {
        Ok(ZatelIpcMessage::new(ZatelIpcData::SaveConfReply(ztl_con)))
    }
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

fn query_all() -> Result<ZatelIpcMessage, ZatelError> {
    let conf_dir_path = std::path::Path::new(CONF_FOLDER);
    let mut ztl_cons: Vec<ZatelConnection> = Vec::new();
    match std::fs::read_dir(CONF_FOLDER) {
        Ok(dir) => {
            for entry in dir {
                let file_path = match entry {
                    Ok(f) => conf_dir_path.join(f.path()),
                    Err(e) => {
                        eprintln!("FAIL: Failed to read dir entry: {}", e);
                        continue;
                    }
                };
                let file_path = match file_path.to_str() {
                    Some(f) => f,
                    None => {
                        eprintln!(
                            "BUG: Should never happen: \
                        file_path.to_str() return None"
                        );
                        continue;
                    }
                };

                let conn_str = match read_file(file_path) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!(
                            "ERROR: Failed to read file {}: {}",
                            file_path, e
                        );
                        continue;
                    }
                };
                let ztl_con: ZatelConnection =
                    match zatel_connection_from_flat_string(&conn_str) {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!(
                                "ERROR: Invalid connection YAML file {}: {}",
                                file_path, e
                            );
                            continue;
                        }
                    };
                ztl_cons.push(ztl_con);
            }
            Ok(ZatelIpcMessage::new(ZatelIpcData::QuerySavedConfAllReply(
                ztl_cons,
            )))
        }
        Err(e) => Err(ZatelError::plugin_error(format!(
            "Failed to read dir {}: {}",
            CONF_FOLDER, e
        ))),
    }
}

fn read_file(file_path: &str) -> Result<String, ZatelError> {
    let mut fd = match std::fs::File::open(file_path) {
        Ok(f) => f,
        Err(e) => {
            return Err(ZatelError::plugin_error(format!(
                "Failed to open {}: {}",
                file_path, e
            )))
        }
    };
    let mut contents = String::new();
    if let Err(e) = fd.read_to_string(&mut contents) {
        Err(ZatelError::plugin_error(format!(
            "Failed to open {}: {}",
            file_path, e
        )))
    } else {
        Ok(contents)
    }
}

fn zatel_connection_to_flat_string(
    ztl_con: &ZatelConnection,
) -> Result<String, ZatelError> {
    let uuid = match &ztl_con.uuid {
        Some(u) => u,
        None => {
            return Err(ZatelError::bug(format!(
                "Got None uuid from daemon for connection {:?}",
                &ztl_con,
            )))
        }
    };
    let name = match &ztl_con.name {
        Some(u) => u,
        None => {
            return Err(ZatelError::bug(format!(
                "Got None name from daemon for connection {:?}",
                &ztl_con,
            )))
        }
    };
    let mut yaml_map: serde_yaml::Mapping =
        match serde_yaml::from_str(&ztl_con.config) {
            Ok(o) => o,
            Err(e) => {
                return Err(ZatelError::bug(format!(
                    "This should never happen, \
                    got invalid YAML file from daemon for SaveConf: {}: {}",
                    &ztl_con.config, e
                )));
            }
        };
    yaml_map.insert(
        serde_yaml::Value::String(CONNECTION_KEY.to_string()),
        gen_connection_setting(uuid, name),
    );
    match serde_yaml::to_string(&yaml_map) {
        Ok(s) => Ok(s),
        Err(e) => Err(ZatelError::bug(format!(
            "This should never happen, \
                failed to generate yaml string from Mapping: {:?}: {}",
            &yaml_map, e
        ))),
    }
}

fn zatel_connection_from_flat_string(
    conn_str: &str,
) -> Result<ZatelConnection, ZatelError> {
    let mut yaml_map: serde_yaml::Mapping = match serde_yaml::from_str(conn_str)
    {
        Ok(o) => o,
        Err(e) => {
            return Err(ZatelError::invalid_argument(format!(
                "Corrupted connection YAML file from disk: {}: {}",
                conn_str, e
            )));
        }
    };
    let conn_setting = match yaml_map
        .remove(&serde_yaml::Value::String(CONNECTION_KEY.to_string()))
    {
        Some(c) => c,
        None => {
            return Err(ZatelError::invalid_argument(format!(
                "connection YAML file does not have section for {}: {}",
                CONNECTION_KEY, conn_str
            )));
        }
    };
    let conn_setting = match conn_setting.as_mapping() {
        Some(m) => m,
        None => {
            return Err(ZatelError::invalid_argument(format!(
                "connection YAML file section {} is not a map: {}",
                CONNECTION_KEY, conn_str
            )));
        }
    };
    let uuid_key = serde_yaml::Value::String("uuid".to_string());
    let name_key = serde_yaml::Value::String("name".to_string());
    if !conn_setting.contains_key(&uuid_key)
        || !conn_setting.contains_key(&name_key)
    {
        return Err(ZatelError::invalid_argument(format!(
            "connection YAML file does not have name or uuid in section \
            {}: {}",
            CONNECTION_KEY, conn_str
        )));
    }
    let uuid = match conn_setting.get(&uuid_key) {
        Some(u) => match u.as_str() {
            Some(s) => s,
            None => {
                return Err(ZatelError::invalid_argument(format!(
                    "connection YAML file does not have \
                    invalid uuid in section {}: {:?}",
                    CONNECTION_KEY, conn_setting
                )));
            }
        },
        None => {
            return Err(ZatelError::invalid_argument(format!(
                "connection YAML file does not have \
                    uuid in section {}: {:?}",
                CONNECTION_KEY, conn_setting
            )));
        }
    };
    let name = match conn_setting.get(&name_key) {
        Some(u) => match u.as_str() {
            Some(s) => s,
            None => {
                return Err(ZatelError::invalid_argument(format!(
                    "connection YAML file does not have \
                    invalid name in section {}: {:?}",
                    CONNECTION_KEY, conn_setting
                )));
            }
        },
        None => {
            return Err(ZatelError::invalid_argument(format!(
                "connection YAML file does not have \
                    name in section {}: {:?}",
                CONNECTION_KEY, conn_setting
            )));
        }
    };
    let config_str = match serde_yaml::to_string(&yaml_map) {
        Ok(s) => s,
        Err(e) => {
            return Err(ZatelError::bug(format!(
                "This should never happen, \
                serde_yaml::to_string(yaml_map) failed: {:?} {}",
                yaml_map, e,
            )));
        }
    };
    Ok(ZatelConnection {
        name: Some(name.to_string()),
        uuid: Some(uuid.to_string()),
        config: config_str,
    })
}

fn gen_connection_setting(uuid: &str, name: &str) -> serde_yaml::Value {
    let mut setting = serde_yaml::Mapping::new();
    setting.insert(
        serde_yaml::Value::String("uuid".to_string()),
        serde_yaml::Value::String(uuid.to_string()),
    );
    setting.insert(
        serde_yaml::Value::String("name".to_string()),
        serde_yaml::Value::String(name.to_string()),
    );
    serde_yaml::Value::Mapping(setting)
}

fn query(uuid: &str) -> Result<ZatelIpcMessage, ZatelError> {
    let conn_str = read_file(&gen_file_path(uuid))?;
    Ok(ZatelIpcMessage::new(ZatelIpcData::QuerySavedConfReply(
        zatel_connection_from_flat_string(&conn_str)?,
    )))
}

fn gen_file_path(uuid: &str) -> String {
    format!("{}/{}{}", CONF_FOLDER, uuid, CONN_FILE_POSTFIX)
}
