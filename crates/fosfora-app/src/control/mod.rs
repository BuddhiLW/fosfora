//! Control ops: every action the UI can take, as a named op any in-process
//! client (the embedded nREPL first) can call.
//!
//! Strata, bottom up:
//!
//! - [`args`]: Promote. JSON args into domain values (`pick` by index or name).
//! - `ops::<domain>::port`: the role-sized port each domain needs from the
//!   app (`PresetPort`, `ScenePort`, ...), implemented for [`App`] and, in
//!   tests, for stubs.
//! - `ops::<domain>`: op bodies generic over their port: Collect (read the
//!   port), Pipeline (pure plan), Boundary (one port call that acts).
//! - this module: the registry and the queue. Generic over the context `C`
//!   ops run against; the app instantiates it with `C = App`.
//!
//! Calls arrive as [`OpCall`]s on an [`OpQueue`]; the app drains them once
//! per frame on the render thread ([`App::run_control_ops`]).

pub mod args;
pub mod ops;

use std::collections::BTreeMap;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use serde_json::{Value, json};

use crate::app::App;

/// What an op returns: JSON on success, a human-readable message on failure.
pub type OpResult = Result<Value, String>;

/// An op body over context `C`. A plain `fn`, so the registry hands out a
/// copy and the context is borrowed mutably only while it runs.
pub type OpFn<C> = fn(&mut C, &Value) -> OpResult;

/// One op: a stable `domain/verb` name, an args sketch and a one-line doc
/// (both shown by `(f/ops)`), and its body.
pub struct OpSpec<C = App> {
    pub name: &'static str,
    pub args: &'static str,
    pub doc: &'static str,
    pub run: OpFn<C>,
}

impl<C> Clone for OpSpec<C> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<C> Copy for OpSpec<C> {}

impl<C> OpSpec<C> {
    pub const fn new(
        name: &'static str,
        args: &'static str,
        doc: &'static str,
        run: OpFn<C>,
    ) -> Self {
        Self {
            name,
            args,
            doc,
            run,
        }
    }
}

/// A clojurust namespace an addon ships for the REPL: preloaded at boot and
/// required into `user` as `alias`.
#[derive(Clone, Copy, Debug)]
pub struct CljrsNs {
    pub ns: &'static str,
    pub alias: &'static str,
    pub src: &'static str,
}

/// A bundle of ops over context `C`. Built-in addons live in [`ops`]; an
/// embedder adds its own with [`App::register_addon`].
pub trait ControlAddon<C = App>: Send + Sync {
    /// Stable id, e.g. `"preset"`.
    fn id(&self) -> &'static str;
    /// The ops this addon contributes.
    fn ops(&self) -> Vec<OpSpec<C>>;
    /// Optional REPL namespace wrapping the ops.
    fn cljrs(&self) -> Option<CljrsNs> {
        None
    }
    /// The UI actions these ops mirror, as `(signal, op)`: `signal` is the
    /// egui temp id the panel raises (`save_preset`), `op` the op that does
    /// the same (`preset/save`). Read by the coverage gate (`coverage.rs`).
    fn ui_signals(&self) -> &'static [(&'static str, &'static str)] {
        &[]
    }
}

/// Every registered op, by name, with the addon that registered it.
pub struct Registry<C = App> {
    ops: BTreeMap<&'static str, (&'static str, OpSpec<C>)>,
    addons: Vec<&'static str>,
}

impl<C> Default for Registry<C> {
    fn default() -> Self {
        Self {
            ops: BTreeMap::new(),
            addons: Vec::new(),
        }
    }
}

impl<C> Registry<C> {
    pub fn from_addons(addons: &[Box<dyn ControlAddon<C>>]) -> Self {
        let mut reg = Self::default();
        for addon in addons {
            reg.register(addon.as_ref());
        }
        reg
    }

    /// Add an addon's ops. A later op with an existing name replaces it, so
    /// an addon can override a built-in.
    pub fn register(&mut self, addon: &dyn ControlAddon<C>) {
        let id = addon.id();
        for op in addon.ops() {
            if let Some((prev, _)) = self.ops.insert(op.name, (id, op)) {
                log::info!("control: addon {id} replaced op {} from {prev}", op.name);
            }
        }
        if !self.addons.contains(&id) {
            self.addons.push(id);
        }
    }

    pub fn get(&self, name: &str) -> Option<OpFn<C>> {
        self.ops.get(name).map(|(_, op)| op.run)
    }

    pub fn addon_ids(&self) -> &[&'static str] {
        &self.addons
    }

    /// `[{name, addon, args, doc}]`, sorted by name.
    pub fn catalog(&self) -> Value {
        Value::Array(
            self.ops
                .values()
                .map(|(addon, op)| {
                    json!({"name": op.name, "addon": addon, "args": op.args, "doc": op.doc})
                })
                .collect(),
        )
    }
}

/// A context that carries its own registry, so ops (`ops`, `addons`) can
/// read it while they run.
pub trait OpHost: Sized {
    fn registry(&self) -> &Registry<Self>;
}

/// Run `name` against `host`. The op's `fn` is copied out of the registry
/// first, so the host is borrowed mutably only by the op itself.
pub fn run<C: OpHost>(host: &mut C, name: &str, args: &Value) -> OpResult {
    match host.registry().get(name) {
        Some(op) => op(host, args),
        None => Err(format!(
            "unknown op {name:?}; (f/ops) lists the registered ones"
        )),
    }
}

/// Run one queued call and answer it.
pub fn dispatch<C: OpHost>(host: &mut C, call: OpCall) {
    let result = run(host, &call.name, &call.args);
    if let Err(ref e) = result {
        log::debug!("control op {} failed: {e}", call.name);
    }
    let _ = call.reply.send(result);
}

impl Registry<App> {
    /// The registry with every built-in addon.
    pub fn with_builtins() -> Self {
        Self::from_addons(&ops::builtin_addons())
    }
}

/// One queued call. The caller blocks on `reply`.
pub struct OpCall {
    pub name: String,
    pub args: Value,
    pub reply: Sender<OpResult>,
}

/// Producer side of the op queue. Cheap to clone; one per client.
#[derive(Clone)]
pub struct OpQueue {
    tx: Sender<OpCall>,
}

impl OpQueue {
    /// A queue and the receiver the app drains.
    pub fn new() -> (Self, Receiver<OpCall>) {
        let (tx, rx) = crossbeam_channel::bounded(256);
        (Self { tx }, rx)
    }

    /// Queue `name` and wait up to `timeout` for the render thread to run it.
    pub fn call(&self, name: &str, args: Value, timeout: Duration) -> OpResult {
        let (reply, answer) = crossbeam_channel::bounded(1);
        self.tx
            .try_send(OpCall {
                name: name.to_owned(),
                args,
                reply,
            })
            .map_err(|e| format!("control queue: {e}"))?;
        answer.recv_timeout(timeout).map_err(|_| {
            format!("op {name}: no answer within {timeout:?} (is the window rendering?)")
        })?
    }
}

impl OpHost for App {
    fn registry(&self) -> &Registry<Self> {
        &self.control
    }
}

impl App {
    /// Register an addon's ops at runtime. Its REPL namespace, if any, is not
    /// loaded (the REPL boots first); call its ops with `(f/op ...)`.
    pub fn register_addon(&mut self, addon: &dyn ControlAddon) {
        self.control.register(addon);
    }

    /// Run every queued op call. Called once per frame from [`App::update`].
    pub fn run_control_ops(&mut self) {
        let calls: Vec<OpCall> = self.web.control_calls().collect();
        for call in calls {
            dispatch(self, call);
        }
    }
}

#[cfg(test)]
mod coverage;
#[cfg(test)]
pub(crate) mod testkit;
#[cfg(test)]
mod tests;
