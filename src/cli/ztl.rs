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

use std::io::Read;

use clap::{App, Arg, ArgMatches, SubCommand};
use zatel::{
    ipc_connect, ipc_exec, ZatelConnection, ZatelIpcData, ZatelIpcMessage,
};

#[tokio::main]
async fn main() {
    let matches = App::new("ztl")
        .about("CLI to Zatel daemon")
        .subcommand(
            SubCommand::with_name("query")
                .about("Query interface information")
                .alias("q")
                .alias("qu")
                .alias("que")
                .alias("quer")
                .arg(
                    Arg::with_name("iface_name")
                        .index(1)
                        .required(true)
                        .help("print debug information verbosely"),
                ),
        )
        .subcommand(
            SubCommand::with_name("connection")
                .about("network connections")
                .alias("c")
                .alias("co")
                .alias("con")
                .alias("conn")
                .alias("connect")
                .alias("connecti")
                .alias("connectio")
                .subcommand(
                    SubCommand::with_name("show")
                        .about("Show saved network connections")
                        .alias("s")
                        .alias("sh")
                        .alias("sho")
                        .arg(
                            Arg::with_name("conn_id")
                                .index(1)
                                .required(true)
                                //TODO: .multiple(true)
                                .help("show specific connections only"),
                        ),
                )
                .subcommand(
                    SubCommand::with_name("import")
                        .about("Import a new connection from file")
                        .alias("i")
                        .alias("im")
                        .alias("imp")
                        .alias("impo")
                        .alias("impor")
                        .arg(
                            Arg::with_name("file_path")
                                .index(1)
                                .required(true)
                                .help("YAML file for connection to add"),
                        ),
                ),
        )
        .get_matches();

    if let Some(matches) = matches.subcommand_matches("query") {
        handle_query(&matches).await;
    } else if let Some(matches) = matches.subcommand_matches("connection") {
        handle_connection(&matches).await;
    } else {
        eprintln!("TODO: show all network state in brief summery");
    }
}

async fn handle_query(matches: &ArgMatches<'_>) {
    let iface_name = matches.value_of("iface_name").unwrap();
    let mut connection = ipc_connect().await.unwrap();
    match ipc_exec(
        &mut connection,
        &ZatelIpcMessage::new(ZatelIpcData::QueryIfaceInfo(
            iface_name.to_string(),
        )),
    )
    .await
    {
        Ok(ZatelIpcMessage {
            data: ZatelIpcData::QueryIfaceInfoReply(s),
            log: _,
        }) => println!("{}", s),
        Ok(i) => eprintln!("Unknown reply: {:?}", i),
        Err(e) => eprintln!("{}", e),
    }
}

async fn handle_connection(matches: &ArgMatches<'_>) {
    if let Some(matches) = matches.subcommand_matches("show") {
        handle_connection_show(matches.value_of("conn_id").unwrap()).await;
    } else if let Some(matches) = matches.subcommand_matches("import") {
        handle_connection_import(matches.value_of("file_path").unwrap()).await;
    } else {
        handle_connection_show_all().await;
    }
}

async fn handle_connection_import(file_path: &str) {
    let content = read_file(file_path);
    let mut connection = ipc_connect().await.unwrap();
    let ztl_con = ZatelConnection::new(content);
    match ipc_exec(
        &mut connection,
        &ZatelIpcMessage::new(ZatelIpcData::SaveConf(ztl_con)),
    )
    .await
    {
        Ok(r) => {
            if let ZatelIpcData::SaveConfReply(new_ztl_con) = &r.data {
                println!("Connection saved:");
                print_connection(&new_ztl_con);
                println!("");
            } else {
                eprintln!("Unexpected reply {:?}", r);
            }
        }
        Err(e) => eprintln!("{}", e),
    }
}

fn read_file(file_path: &str) -> String {
    let mut fd = std::fs::File::open(file_path).expect("Failed to open file");
    let mut contents = String::new();
    fd.read_to_string(&mut contents)
        .expect("Failed to read file");
    contents
}

async fn handle_connection_show_all() {
    let mut connection = ipc_connect().await.unwrap();
    match ipc_exec(
        &mut connection,
        &ZatelIpcMessage::new(ZatelIpcData::QuerySavedConfAll),
    )
    .await
    {
        Ok(r) => {
            if let ZatelIpcData::QuerySavedConfAllReply(ztl_cons) = r.data {
                println!("{:>36} | name", "UUID              ");
                for ztl_con in ztl_cons {
                    println!(
                        "{}   {}",
                        ztl_con.uuid.as_ref().unwrap(),
                        ztl_con.name.as_ref().unwrap()
                    );
                }
                println!("");
            } else {
                eprintln!("Unknown reply {:?}", r);
            }
        }
        Err(e) => eprintln!("{}", e),
    }
}

fn print_connection(ztl_con: &ZatelConnection) {
    println!("connection UUID: {}", ztl_con.uuid.as_ref().unwrap());
    println!("connection name: {}", ztl_con.name.as_ref().unwrap());
    println!("{}", &ztl_con.config);
}

async fn handle_connection_show(uuid: &str) {
    let mut connection = ipc_connect().await.unwrap();
    match ipc_exec(
        &mut connection,
        &ZatelIpcMessage::new(ZatelIpcData::QuerySavedConf(uuid.to_string())),
    )
    .await
    {
        Ok(r) => {
            if let ZatelIpcData::QuerySavedConfReply(ztl_con) = &r.data {
                print_connection(&ztl_con);
                println!("");
            } else {
                eprintln!("Unexpected reply {:?}", r);
            }
        }
        Err(e) => eprintln!("{}", e),
    }
}
