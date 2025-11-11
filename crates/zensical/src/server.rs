// Copyright (c) 2025 Zensical and contributors

// SPDX-License-Identifier: MIT
// Third-party contributions licensed under DCO

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
use std::sync::Arc;
use std::{fs, thread};
use zensical_serve::handler::Stack;
use zensical_serve::middleware;
use zensical_serve::server::{Result, Server};

use super::config::Config;

mod client;

use client::Client;

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Creates an HTTP server to serve the site.
pub fn create_server(
    config: &Config, receiver: Receiver<String>, addr: Option<String>,
) -> Arc<Waker> {
    let site_dir = config.get_site_dir();
    fs::create_dir_all(&site_dir).expect("site directory could not be created");

    // Create a one shot channel to extract waker - this is currently necessary,
    // so that the server wakes up when the file watcher emits new events
    let (tx, rx) = unbounded();

    // Create new thread to run the server
    let base = config.get_base_path();
    let addr = addr.unwrap_or_else(|| config.project.dev_addr.clone());
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

            // Start server and extract waker for interaction with event loop
            let mut server = match Server::new(stack, addr) {
                Ok(server) => server,
                Err(err) => {
                    let _ = tx.send(Err(err));
                    return Ok(());
                }
            };
            let _ = tx.send(Ok(server.waker()));
            loop {
                server.poll(Some(&receiver))?;
            }
        }
    });

    // Return waker, or fail if server thread could not be started - we need to
    // restructure this logic, but for now, it's quite safe to assume that when
    // the server thread could not be started, the address is already in use.
    match rx.recv().expect("invariant") {
        Ok(waker) => waker,
        Err(err) => {
            eprintln!("Error: {err}");
            std::process::exit(1);
        }
    }
}
