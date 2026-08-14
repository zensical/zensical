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

//! Stream aggregation for rebuildable site-wide snapshots.

use ahash::HashSet;
use std::marker::PhantomData;
use zrx::id::{id, Id};
use zrx::scheduler::action::context::Binding;
use zrx::scheduler::action::options::{Event, Interest};
use zrx::scheduler::action::{Action, Context, Options};
use zrx::scheduler::schedule::Subscriber;
use zrx::scheduler::step::{IntoSteps, Scope};
use zrx::scheduler::{Key, Value};
use zrx::stream::operator::Operator;
use zrx::stream::{Barrier, Stream};

// ----------------------------------------------------------------------------
// Structs
// ----------------------------------------------------------------------------

/// Aggregate all values matching a barrier into one site-wide snapshot.
struct Aggregate<T> {
    /// Base output scope.
    output: Key<Id>,
    /// Barrier selecting source scopes.
    barrier: Barrier<Id>,
    /// Source scopes that have not reached this stream yet.
    pending: HashSet<Key<Id>>,
    /// Current output scope.
    current: Option<Key<Id>>,
    /// Output generation.
    generation: u64,
    /// Capture value type.
    marker: PhantomData<T>,
}

// ----------------------------------------------------------------------------
// Implementations
// ----------------------------------------------------------------------------

impl<T> Aggregate<T> {
    /// Create an aggregate for the given output scope and barrier.
    fn new(output: Key<Id>, barrier: Barrier<Id>) -> Self {
        Self {
            output,
            barrier,
            pending: HashSet::default(),
            current: None,
            generation: 0,
            marker: PhantomData,
        }
    }
}

// ----------------------------------------------------------------------------
// Trait implementations
// ----------------------------------------------------------------------------

impl<T> Action<Id> for Aggregate<T>
where
    T: Value + Clone,
{
    type Inputs = (T,);
    type Output<'a> = Vec<(Key<Id>, T)>;

    fn execute(&mut self, ctx: Context<Id, Self>) -> impl IntoSteps<Id, Self> {
        let Binding {
            events,
            scopes,
            inputs,
            mut output,
            ..
        } = ctx.bind();
        // Track every submitted source scope, including repeated submissions
        // of an existing scope during serve rebuilds.
        for event in events {
            match event {
                Event::Insert(scope) if self.barrier.contains(&scope) => {
                    self.pending.insert(scope);
                }
                Event::Remove(scope) => {
                    self.pending.remove(&scope);
                }
                Event::Insert(_) => {}
            }
        }

        // A scope reaching this action is complete for this stream. Repeated
        // scopes must still advance the aggregate, even if they were already
        // present during a previous build.
        let mut advanced = false;
        for scope in scopes {
            if self.barrier.contains(scope.key()) {
                self.pending.remove(scope.key());
                advanced = true;
            }
        }

        let complete = advanced && self.pending.is_empty();
        let mut steps = Vec::new();
        if complete {
            let mut values = inputs
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Vec<_>>();
            values.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));

            // A source submission can reach the aggregate before an updated
            // asynchronous value does. Ignore that lifecycle-only advance;
            // the changed value will arrive in a subsequent scope.
            if self.current.as_ref().and_then(|key| output.get(key))
                == Some(&values)
            {
                return steps.into_iter();
            }

            // Repeated synthetic scopes are not propagated by the current
            // runtime. Rotate the aggregate scope on every snapshot, removing
            // the previous generation so downstream stores stay bounded.
            self.generation =
                self.generation.checked_add(1).expect("invariant");
            let id = id!(
                self.output.try_as_id().expect("invariant");
                fragment = self.generation.to_string()
            )
            .expect("invariant");
            let key = Key::from(id);

            if let Some(current) = self.current.replace(key.clone()) {
                output.remove(&current);
                steps.push(Scope::from(current).done());
            }
            output.insert(key.clone(), values);
            steps.push(Scope::from(key).done());
        }
        steps.into_iter()
    }
}

// ----------------------------------------------------------------------------
// Functions
// ----------------------------------------------------------------------------

/// Aggregate a stream whenever all matching source scopes have completed.
pub fn aggregate<T>(
    stream: &Stream<Id, T>, (output, barrier): (Key<Id>, Barrier<Id>),
) -> Stream<Id, Vec<(Key<Id>, T)>>
where
    T: Value + Clone,
{
    let options = Options::default()
        .interest(Interest::Enter)
        .interest(Interest::Leave);
    stream.subscribe(
        Subscriber::new(Aggregate::new(output, barrier)).with_options(options),
    )
}

// ----------------------------------------------------------------------------
// Tests
// ----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use zrx::module::Context as ModuleContext;
    use zrx::scheduler::Scheduler;
    use zrx::stream::Workflow;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Number(u8);

    impl Value for Number {}

    fn tick_until(
        scheduler: &mut Scheduler<Id>, condition: impl Fn() -> bool,
        stage: &str,
    ) {
        for _ in 0..100 {
            if condition() {
                return;
            }
            scheduler.tick_timeout(Duration::from_millis(10)).unwrap();
        }
        panic!("scheduler did not produce output after {stage}");
    }

    #[test]
    fn test_aggregate_reemits_changed_repeated_scope() {
        let context = ModuleContext::default();
        let input = context.add::<Number>();
        let root = Key::from(
            id!(provider = "file", context = ".", location = ".").unwrap(),
        );
        let barrier =
            Barrier::new(|key: &Key<Id>| key[0].location().as_ref() == "item");
        let snapshots = aggregate(&input, (root, barrier));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_stream = Arc::clone(&seen);
        snapshots.map(move |values: Vec<(Key<Id>, Number)>| {
            let (_, Number(value)) = values.first().expect("invariant");
            seen_by_stream.lock().expect("invariant").push(*value);
        });
        drop(snapshots);
        drop(input);

        let workflow: Workflow<Id> = context.into();
        let mut scheduler = Scheduler::<Id>::default();
        scheduler.attach(workflow);
        let session = scheduler.session::<Number>();
        let item =
            id!(provider = "file", context = ".", location = "item").unwrap();

        session.insert(item.clone(), Number(1)).unwrap();
        tick_until(
            &mut scheduler,
            || seen.lock().expect("invariant").last() == Some(&1),
            "first insertion",
        );
        session.insert(item, Number(2)).unwrap();
        tick_until(
            &mut scheduler,
            || seen.lock().expect("invariant").last() == Some(&2),
            "changed insertion",
        );

        assert_eq!(*seen.lock().expect("invariant"), [1, 2]);
    }
}
