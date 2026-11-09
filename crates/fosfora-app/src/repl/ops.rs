//! Control ops from the REPL: the `op` native, the addon namespaces preloaded
//! at boot, and the user's own init scripts.
//!
//! `(native/op "preset/save" "{:name \"x\"}")` queues the op for the render
//! thread, blocks until it ran (or [`OP_TIMEOUT`]), and returns its result as
//! EDN text; a failed op throws with the op's message.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cljrs_runtime::tiered::{Env, GlobalEnv};
use cljrs_value::{Arity, ValueError};

use super::{def_native, edn_to_json, json_to_edn, str_arg, str_value};
use crate::control::{CljrsNs, OpQueue};

/// How long a REPL call waits for the render thread. Long enough for a
/// preset load that compiles shaders; short enough to notice a stalled app.
pub(super) const OP_TIMEOUT: Duration = Duration::from_secs(10);

pub(super) fn register(globals: &GlobalEnv, ops: OpQueue) {
    def_native(globals, "op", Arity::Fixed(2), move |args| {
        let name = str_arg(args, 0)?;
        let edn = str_arg(args, 1)?;
        let json = edn_to_json(&edn).map_err(ValueError::Other)?;
        let result = ops.call(&name, json, OP_TIMEOUT).map_err(ValueError::Other)?;
        let mut out = String::new();
        json_to_edn(&result, &mut out);
        Ok(str_value(out))
    });
}

/// The `(require ...)` that brings every addon namespace into `user` under
/// its alias.
pub(super) fn require_form(namespaces: &[CljrsNs]) -> Option<String> {
    if namespaces.is_empty() {
        return None;
    }
    let specs: Vec<String> = namespaces
        .iter()
        .map(|n| format!("'[{} :as {}]", n.ns, n.alias))
        .collect();
    Some(format!("(require {})", specs.join(" ")))
}

/// Where user REPL scripts live: every `*.cljrs` here is evaluated into
/// `user` at boot, in name order. `FOSFORA_REPL_INIT` names another folder.
pub(super) fn init_dir() -> PathBuf {
    std::env::var_os("FOSFORA_REPL_INIT")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::paths::config_root().join("repl"))
}

/// The init scripts in `dir`, sorted; none when the folder is missing.
pub(super) fn init_scripts(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "cljrs"))
        .collect();
    files.sort();
    files
}

/// Evaluate every init script. A broken script is logged and skipped, so one
/// typo never keeps the REPL from starting.
pub(super) fn load_init_scripts(env: &mut Env) {
    for path in init_scripts(&init_dir()) {
        let result = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|src| super::eval_str(env, &src));
        match result {
            Ok(()) => log::info!("nREPL: loaded {}", path.display()),
            Err(e) => log::warn!("nREPL: {} failed: {e:#}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_require_form_aliases_every_namespace() {
        let ns = [
            CljrsNs { ns: "fosfora.preset", alias: "preset", src: "" },
            CljrsNs { ns: "fosfora.bindings", alias: "bind", src: "" },
        ];
        assert_eq!(
            require_form(&ns).unwrap(),
            "(require '[fosfora.preset :as preset] '[fosfora.bindings :as bind])"
        );
        assert_eq!(require_form(&[]), None);
    }

    #[test]
    fn init_scripts_are_cljrs_files_in_name_order() {
        let dir = tempfile::tempdir().unwrap();
        for f in ["b.cljrs", "a.cljrs", "notes.txt"] {
            std::fs::write(dir.path().join(f), "").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub.cljrs")).unwrap();
        let names: Vec<_> = init_scripts(dir.path())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.cljrs", "b.cljrs"]);
        assert!(init_scripts(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn every_builtin_namespace_parses() {
        for ns in crate::control::ops::builtin_cljrs() {
            let forms = cljrs_reader::Parser::new(ns.src.to_owned(), ns.ns.to_owned())
                .parse_all()
                .unwrap_or_else(|e| panic!("{} does not parse: {e:?}", ns.ns));
            assert!(!forms.is_empty(), "{} is empty", ns.ns);
        }
    }
}
