use std::{collections::HashMap, fmt::Debug, hash::Hash};

use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};

#[cfg(test)]
#[path = "./graph.test.rs"]
mod graph_tests;

pub trait GraphCache<TIn, TOut> where Self: Sized {
    fn graph_cache(&self, value: &TIn) -> Option<TOut>;
}

pub trait GraphIn<'a, TCtx, TOut, TErr> where Self: Sized, TCtx: Send {
    fn graph_dependencies(&self, dependencies: &Vec<&TOut>) -> Vec<Self>;
    fn graph_run(self, ctx: TCtx, dependencies: Vec<TOut>) -> impl std::future::Future<Output = Result<TOut, TErr>> + Send + 'a;
}

pub trait GraphOut<TIn> where Self: Sized {
    fn graph_follow_ups(&self) -> Vec<TIn>;
}

pub struct GraphTaskResults<TIn, TOut, TErr> {
    success: HashMap<TIn, TOut>,
    failed: Vec<(TIn, TErr)>,
}

impl<TIn, TOut, TErr> GraphTaskResults<TIn, TOut, TErr> {
    pub fn new() -> Self {
        Self {
            success: HashMap::new(),
            failed: Vec::new(),
        }
    }

    pub fn get_failed(&self) -> Option<&Vec<(TIn, TErr)>> {
        if self.failed.is_empty() {
            None
        } else {
            Some(&self.failed)
        }
    }

    pub fn unwrap(self) -> HashMap<TIn, TOut> {
        assert!(self.failed.is_empty());

        self.success
    }

    pub fn ok_or<E>(self, err: E) -> Result<HashMap<TIn, TOut>, E> {
        if self.failed.len() > 0 {
            Err(err)
        } else {
            Ok(self.success)
        }
    }

    pub fn ok_or_else<E, F: FnOnce(Vec<(TIn, TErr)>) -> E>(self, f: F) -> Result<HashMap<TIn, TOut>, E> {
        if self.failed.len() > 0 {
            Err(f(self.failed))
        } else {
            Ok(self.success)
        }
    }
}

pub struct GraphTasks<'a, TCtx, TIn, TOut, TErr, TCache> {
    context: TCtx,
    cache: TCache,

    ready: Vec<TIn>,
    running: FuturesUnordered<BoxFuture<'a, (TIn, Result<TOut, TErr>)>>,
    results: GraphTaskResults<TIn, TOut, TErr>,

    tasks: HashMap<TIn, (usize, Vec<TIn>)>,
    dependents: HashMap<TIn, Vec<TIn>>,
}

impl<'a, TCtx, TIn, TOut, TErr, TCache> GraphTasks<'a, TCtx, TIn, TOut, TErr, TCache> where
    TCtx: Clone + Send,
    TIn: Clone + Debug + Eq + Hash + Send + GraphIn<'a, TCtx, TOut, TErr> + 'a,
    TOut: Clone + GraphOut<TIn>,
    TCache: GraphCache<TIn, TOut>
{
    pub fn new(context: TCtx, cache: TCache) -> Self {
        Self {
            context,
            cache,

            ready: Vec::new(),
            running: FuturesUnordered::new(),
            results: GraphTaskResults::new(),

            tasks: HashMap::new(),
            dependents: HashMap::new(),
        }
    }

    pub fn register(&mut self, op: TIn) {
        if !self.tasks.contains_key(&op) {
            let dependencies
                = op.graph_dependencies(&vec![]);

            if dependencies.is_empty() {
                self.tasks.insert(op.clone(), (0, vec![]));

                self.try_ready(op);
            } else {
                let resolved_dependency_count = dependencies.iter()
                    .filter(|dep| self.results.success.contains_key(dep))
                    .count();

                self.tasks.insert(op.clone(), (resolved_dependency_count, dependencies.clone()));

                if resolved_dependency_count == dependencies.len() {
                    self.try_ready(op.clone());
                }

                for dependency in &dependencies {
                    self.dependents.entry(dependency.clone())
                        .or_default()
                        .push(op.clone());

                    self.register(dependency.clone());
                }
            }
        }
    }

    fn try_ready(&mut self, op: TIn) {
        loop {
            let (resolved_dependency_count, dependency_ops) = self.tasks.get_mut(&op)
                .expect("Expected the task entry to exist for ops registered in the ready list");

            let resolved_dependencies = dependency_ops
                .iter()
                .filter_map(|dep| self.results.success.get(dep))
                .collect::<Vec<_>>();

            // If we're missing any of the dependency results we must wait for
            // them to be resolved before we can proceed with the scheduling.
            if resolved_dependencies.len() != dependency_ops.len() {
                *resolved_dependency_count = resolved_dependencies.len();
                return;
            }

            let next_dependencies
                = op.graph_dependencies(&resolved_dependencies);

            // If no new dependency has been added it means that everything
            // needed has been resolved and we can just go on with scheduling
            // the operation for evaluation.
            if dependency_ops.len() == next_dependencies.len() {
                break;
            }

            let previous_dependency_count
                = std::mem::replace(dependency_ops, next_dependencies.clone())
                    .len();

            for dependency in &next_dependencies[previous_dependency_count..] {
                self.dependents.entry(dependency.clone())
                    .or_default()
                    .push(op.clone());

                self.register(dependency.clone());
            }
        }

        self.ready.push(op);
    }

    fn update(&mut self) {
        while self.running.len() < 100 {
            if let Some(op) = self.ready.pop() {
                if let Some(cached_value) = self.cache.graph_cache(&op) {
                    self.accept_cached(op, cached_value);
                    continue;
                }

                let (resolved_dependency_count, dependencies) = self.tasks.get(&op)
                    .expect("Expected the task entry to exist for ops registered in the ready list");

                assert_eq!(*resolved_dependency_count, dependencies.len());

                let op_dependencies = dependencies.iter()
                    .map(|dep| self.results.success.get(dep).cloned().expect("Expected a resolved dependency to have a success status"))
                    .collect();

                let op_clone = op.clone();
                let op_run
                    = op.graph_run(self.context.clone(), op_dependencies);

                let op_run_tagged
                    = op_run.map(move |x| (op_clone, x))
                        .boxed();

                self.running.push(op_run_tagged);
            } else {
                break;
            }
        }
    }

    // This method is just here to make stacktraces contain information about
    // whether or not a task was accepted as a cached value or not.
    fn accept_cached(&mut self, op: TIn, out: TOut) {
        self.accept(op, out);
    }

    pub fn accept(&mut self, op: TIn, out: TOut) {
        let follow_ups = out.graph_follow_ups();

        self.results.success.insert(op.clone(), out);

        if let Some(dependents) = self.dependents.remove(&op) {
            for dependent in dependents {
                let (resolved_dependency_count, dependencies) = self.tasks.get_mut(&dependent)
                    .expect("Expected the task entry to exist for ops registered as dependents");

                for dependency in dependencies.iter_mut() {
                    if dependency == &op {
                        *resolved_dependency_count += 1;
                    }
                }

                if *resolved_dependency_count == dependencies.len() {
                    self.try_ready(dependent.clone());
                }
            }
        }

        for follow_up in follow_ups {
            self.register(follow_up);
        }
}

    pub async fn run(mut self) -> GraphTaskResults<TIn, TOut, TErr> {
        self.update();

        while let Some((op, res)) = self.running.next().await {
            match res {
                Ok(out) => {
                    self.accept(op, out);
                },

                Err(err) => {
                    self.results.failed.push((op, err));
                },
            }

            self.update();
        }

        self.results
    }
}
