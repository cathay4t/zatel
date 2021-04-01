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

mod connection;
mod error;
mod ipc;
mod logging;
mod plugin;
mod yaml;

pub use crate::connection::ZatelConnection;
pub use crate::error::ZatelError;
pub use crate::ipc::{
    ipc_bind, ipc_bind_with_path, ipc_connect, ipc_connect_with_path, ipc_exec,
    ipc_recv, ipc_recv_safe, ipc_send, ZatelIpcData, ZatelIpcMessage,
};
pub use crate::logging::{ZatelLogEntry, ZatelLogLevel};
pub use crate::plugin::{
    ipc_plugin_exec, ipc_plugins_exec, ZatelPluginCapacity, ZatelPluginInfo,
};
pub use crate::yaml::{merge_yaml_lists, merge_yaml_mappings};
