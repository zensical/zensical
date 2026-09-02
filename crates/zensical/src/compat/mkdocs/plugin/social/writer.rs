// Copyright (c) 2025-2026 Zensical and contributors

// SPDX-License-Identifier: MIT
// All contributions are certified under the DCO

// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to
// deal in the Software without restriction, including without limitation the
// rights to use, copy, modify, merge, publish, distribute, sublicense, and/or
// sell copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NON-INFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
// FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS
// IN THE SOFTWARE.

// ----------------------------------------------------------------------------

//! Removal-aware social card output.

use anyhow::anyhow;
use std::fs;
use std::io;
use std::path::PathBuf;

use zrx::id::Id;
use zrx::scheduler::action::{Action, Concurrency, Context};
use zrx::stream::operator::Operator;
use zrx::stream::{Change, Key, Stream};

use crate::path::{OutputRoot, SitePath};

use super::Card;

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

#[derive(Clone)]
struct Writer {
    output: OutputRoot,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl Writer {
    fn path(&self, card: &Card) -> PathBuf {
        self.output.join(&card.path)
    }

    fn insert(&self, card: &Card) -> anyhow::Result<()> {
        let path = self.path(card);
        fs::create_dir_all(path.parent().expect("social card has parent"))?;
        let mut source = fs::File::open(&card.source)?;
        let mut target = fs::File::create(path)?;
        io::copy(&mut source, &mut target)?;
        Ok(())
    }

    fn remove(&self, key: &Key<Id>) -> anyhow::Result<()> {
        let id = key.try_as_id()?;
        let path = id.location().parse::<SitePath>()?;
        let path = self.output.join(&path);
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl Action<Key<Id>> for Writer {
    type Inputs = (Card,);
    type Output = ();

    fn concurrency(&self) -> Concurrency<Self> {
        Concurrency::adaptive()
    }

    fn execute(&mut self, context: Context<'_, Key<Id>, Self>) {
        let Context { inputs: input, output, .. } = context;
        input.for_each(output, |change, emit| {
            match change {
                Change::Insert(key, card) => {
                    let id = key.try_as_id()?;
                    if id.provider() != "file" || id.context() != "." {
                        return Err(anyhow!(
                            "invalid social card output identity"
                        )
                        .into());
                    }
                    self.insert(card.as_ref())?;
                    emit.insert(key, ());
                }
                Change::Remove(key) => {
                    self.remove(&key)?;
                    emit.remove(key);
                }
            }
            Ok(())
        });
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

pub fn setup(output: OutputRoot, cards: &Stream<Id, Card>) {
    let _ = cards.subscribe(Writer { output });
}
