// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:

// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.

// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

// ----------------------------------------------------------------------------

//! Preview server.

use crossbeam::channel::{unbounded, Receiver};
use mio::Waker;
use pyo3::FromPyObject;
use std::sync::Arc;
use std::{fs, thread};

use zensical_serve::handler::Stack;
use zensical_serve::middleware;
use zensical_serve::server::{Result, Server};

use super::config::Config;

mod client;

use client::Client;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Serve options.
#[derive(Clone, Debug, FromPyObject, PartialEq, Eq)]
#[pyo3(from_item_all)]
pub struct ServeOptions {
    /// Address to bind to.
    pub dev_addr: Option<String>,
    /// Whether to open the browser automatically.
    pub open: bool,
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// An HTTP server is created with its waker and advertised addresses returned.
pub fn create_server(
    config: &Config, receiver: Receiver<String>, options: ServeOptions,
) -> (Arc<Waker>, Vec<String>) {
    let site_dir = config.output_root().as_path().to_owned();
    fs::create_dir_all(&site_dir).expect("site directory could not be created");

    // The waker and bound addresses are returned through a one shot channel.
    // The waker is used when new events are emitted by the file watcher.
    let (tx, rx) = unbounded();
    let addr = options
        .dev_addr
        .unwrap_or_else(|| config.project.dev_addr.clone());

    // Create new thread to run the server
    let base = config.get_base_path();
    thread::spawn({
        let tx = tx.clone();
        move || -> Result {
            // Ensure site directory exists
            fs::create_dir_all(&site_dir).unwrap();
            let stack = Stack::new()
                .with(Client::default())
                .with(middleware::WebSocketHandshake::default())
                .with(middleware::NormalizePath::default())
                .with(middleware::BasePath::new(base).expect("invariant"))
                .with(
                    middleware::StaticFiles::new(&site_dir).expect("invariant"),
                );

            // Listener addresses are obtained after the server has been bound.
            let (mut server, addrs) =
                match Server::new(stack, &addr).and_then(|server| {
                    let addrs = server.local_addrs()?;
                    Ok((server, addrs))
                }) {
                    Ok(server) => server,
                    Err(err) => {
                        let _ = tx.send(Err(err));
                        return Ok(());
                    }
                };

            // Concrete addresses are used for ephemeral ports so that each URL
            // reaches the correct listener, even across address families.
            // Configured hostnames are preserved for explicit ports.
            let addrs = if addr
                .rsplit_once(':')
                .is_some_and(|(_, port)| port.parse::<u16>() == Ok(0))
            {
                addrs.iter().map(ToString::to_string).collect()
            } else {
                vec![addr]
            };

            // The first advertised address is opened when requested.
            if options.open {
                let url = format!("http://{}", addrs[0]);
                let _ = webbrowser::open(&url);
            }

            // The waker and advertised addresses are sent to the main thread.
            let _ = tx.send(Ok((server.waker(), addrs)));
            loop {
                server.poll(Some(&receiver))?;
            }
        }
    });

    // Server details are returned after successful binding, or the error is
    // reported if the server thread could not be started.
    match rx.recv().expect("invariant") {
        Ok(server) => server,
        Err(err) => {
            eprintln!("Error: {err}");
            std::process::exit(1);
        }
    }
}
