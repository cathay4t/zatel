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

use tokio::{self, io::AsyncWriteExt, net::UnixStream, task};

use zatel::{ipc_bind, ipc_recv_safe, ipc_send, ZatelIpcCmd, ZatelIpcMessage};

#[tokio::main(flavor = "multi_thread", worker_threads = 50)]
async fn main() {
    let listener = match ipc_bind() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                println!("client connected");
                // TODO: Limit the maximum connected client.
                task::spawn(async move { handle_client(stream).await });
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
        match ipc_recv_safe(&mut stream).await {
            Ok(ZatelIpcMessage { cmd, data }) => {
                if cmd == ZatelIpcCmd::ConnectionClosed {
                    shutdown_connection(&mut stream).await;
                    break;
                } else {
                    println!("got IPC cmd: {:?}: data: {:?}", &cmd, &data);
                    if let Err(e) = ipc_send(
                        &mut stream,
                        &ZatelIpcMessage {
                            cmd: cmd,
                            data: data.clone(),
                        },
                    )
                    .await
                    {
                        eprintln!("Got failure when reply to client: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("IPC error {}", e);
                shutdown_connection(&mut stream).await;
                break;
            }
        }
    }
}
