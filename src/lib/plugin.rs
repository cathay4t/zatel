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

use serde::{Deserialize, Serialize};

use crate::{
    ipc_connect_with_path, ipc_recv, ipc_send, ZatelError,
    ZatelIpcMessage,
};

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ZatelPluginInfo {
    pub name: String,
    pub socket_path: String,
}

pub async fn ipc_plugin_exec(
    plugin_info: &ZatelPluginInfo,
    ipc_msg: &ZatelIpcMessage,
) -> Result<ZatelIpcMessage, ZatelError> {
    let mut stream = ipc_connect_with_path(&plugin_info.socket_path).await?;
    ipc_send(&mut stream, ipc_msg).await?;
    // TODO: Handle timeout
    ipc_recv(&mut stream).await
}
