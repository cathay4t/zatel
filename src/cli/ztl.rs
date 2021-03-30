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

use clap::{App, Arg, SubCommand, AppSettings};
use zatel::{ipc_connect, ipc_exec, ZatelIpcData, ZatelIpcMessage};

#[tokio::main]
async fn main() {
    let matches = App::new("ztl")
        .about("CLI to Zatel daemon")
        .setting(AppSettings::ArgRequiredElseHelp)
        .subcommand(
            SubCommand::with_name("query")
                .about("Query interface information")
                .arg(
                    Arg::with_name("iface_name")
                        .index(1)
                        .help("print debug information verbosely"),
                ),
        )
        .get_matches();

    // You can handle information about subcommands by requesting their matches by name
    // (as below), requesting just the name used, or both at the same time
    if let Some(matches) = matches.subcommand_matches("query") {
        handle_query(matches.value_of("iface_name").unwrap()).await;
    }
}

async fn handle_query(iface_name: &str) {
    let mut connection = ipc_connect().await.unwrap();
    match ipc_exec(
        &mut connection,
        &ZatelIpcMessage::new(ZatelIpcData::QueryIfaceInfo(
            iface_name.to_string(),
        )),
    ).await {
        Ok(ZatelIpcMessage {
            data: ZatelIpcData::QueryIfaceInfoReply(s),
            log: _,
        }) => println!("{}", s),
        Ok(i) => eprintln!("Unknown reply: {:?}", i),
        Err(e) => eprintln!("{}", e),
    }
}
