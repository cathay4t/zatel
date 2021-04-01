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

use crate::{merge_yaml_mappings, ZatelError};

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct ZatelConnection {
    pub uuid: Option<String>,
    pub name: Option<String>,
    pub config: String,
    // More option like auto_connect, auto_connect_ports, volatile and etc
}

impl ZatelConnection {
    pub fn new(config: String) -> Self {
        ZatelConnection {
            uuid: None,
            name: None,
            config: config,
        }
    }
    pub fn merge_from(
        &mut self,
        ztl_cons: &[ZatelConnection],
    ) -> Result<(), ZatelError> {
        let mut conf_strs = Vec::new();
        conf_strs.push(self.config.as_str());
        let self_name = match &self.name {
            Some(n) => n,
            None => {
                return Err(ZatelError::bug(format!(
                    "Self ZatelConnection has no name: {:?}",
                    self
                )))
            }
        };
        let self_uuid = match &self.uuid {
            Some(u) => u,
            None => {
                return Err(ZatelError::bug(format!(
                    "Self ZatelConnection has no uuid: {:?}",
                    self
                )))
            }
        };
        for ztl_con in ztl_cons {
            if let Some(name) = &ztl_con.name {
                if name != self_name {
                    return Err(ZatelError::plugin_error(format!(
                        "WARN: ZatelConnection to merge is holding \
                        different name: origin {}, to merge {:?}",
                        &self_name, &ztl_con.name
                    )));
                }
            }

            if let Some(uuid) = &ztl_con.uuid {
                if uuid != self_uuid {
                    return Err(ZatelError::plugin_error(format!(
                        "WARN: ZatelConnection to merge is holding \
                        different uuid: origin {}, to merge {:?}",
                        &self_uuid, &ztl_con.uuid
                    )));
                }
            }

            conf_strs.push(ztl_con.config.as_str());
        }
        self.config = merge_yaml_mappings(conf_strs.as_slice())?;
        Ok(())
    }
}
