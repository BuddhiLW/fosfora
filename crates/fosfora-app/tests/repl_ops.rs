//! Integration leg of the control-op trifecta: a running fosfora, driven over
//! its embedded nREPL exactly as CIDER would.
//!
//! Skipped unless `FOSFORA_IT_NREPL=<port>` names a live instance started with
//! `FOSFORA_NREPL=<port>` (`--features repl`). It writes and then deletes one
//! uniquely named preset in that instance's config folder.
//!
//!     FOSFORA_NREPL=7888 cargo run --features repl,video &
//!     FOSFORA_IT_NREPL=7888 cargo test --features repl,video --test repl_ops

use std::io::{BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// A bencode value, as far as nREPL needs it.
#[derive(Debug, Clone, PartialEq)]
enum B {
    Int(i64),
    Str(String),
    List(Vec<B>),
    Dict(Vec<(String, B)>),
}

impl B {
    fn get(&self, key: &str) -> Option<&B> {
        match self {
            B::Dict(kv) => kv.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            B::Str(s) => Some(s),
            _ => None,
        }
    }
}

fn encode(v: &B, out: &mut Vec<u8>) {
    match v {
        B::Int(i) => out.extend(format!("i{i}e").bytes()),
        B::Str(s) => {
            out.extend(format!("{}:", s.len()).bytes());
            out.extend(s.bytes());
        }
        B::List(items) => {
            out.push(b'l');
            items.iter().for_each(|i| encode(i, out));
            out.push(b'e');
        }
        B::Dict(kv) => {
            out.push(b'd');
            for (k, v) in kv {
                encode(&B::Str(k.clone()), out);
                encode(v, out);
            }
            out.push(b'e');
        }
    }
}

fn byte(r: &mut impl Read) -> std::io::Result<u8> {
    let mut b = [0u8];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn decode_from(first: u8, r: &mut impl Read) -> std::io::Result<B> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_owned());
    Ok(match first {
        b'i' => {
            let mut s = String::new();
            loop {
                match byte(r)? {
                    b'e' => break,
                    c => s.push(c as char),
                }
            }
            B::Int(s.parse().map_err(|_| bad("int"))?)
        }
        b'l' => {
            let mut items = Vec::new();
            loop {
                match byte(r)? {
                    b'e' => break,
                    c => items.push(decode_from(c, r)?),
                }
            }
            B::List(items)
        }
        b'd' => {
            let mut kv = Vec::new();
            loop {
                match byte(r)? {
                    b'e' => break,
                    c => {
                        let B::Str(k) = decode_from(c, r)? else {
                            return Err(bad("dict key"));
                        };
                        let v = decode_from(byte(r)?, r)?;
                        kv.push((k, v));
                    }
                }
            }
            B::Dict(kv)
        }
        d @ b'0'..=b'9' => {
            let mut len = String::from(d as char);
            loop {
                match byte(r)? {
                    b':' => break,
                    c => len.push(c as char),
                }
            }
            let mut buf = vec![0u8; len.parse().map_err(|_| bad("len"))?];
            r.read_exact(&mut buf)?;
            B::Str(String::from_utf8_lossy(&buf).into_owned())
        }
        _ => return Err(bad("unknown bencode tag")),
    })
}

struct Repl {
    out: TcpStream,
    input: BufReader<TcpStream>,
    next_id: u32,
}

impl Repl {
    fn connect(port: u16) -> Self {
        let out = TcpStream::connect(("127.0.0.1", port)).expect("connect to FOSFORA_IT_NREPL");
        out.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let input = BufReader::new(out.try_clone().unwrap());
        Self { out, input, next_id: 0 }
    }

    /// Evaluate `code`; the printed value, or the error text.
    fn eval(&mut self, code: &str) -> Result<String, String> {
        self.next_id += 1;
        let id = self.next_id.to_string();
        let msg = B::Dict(vec![
            ("op".into(), B::Str("eval".into())),
            ("code".into(), B::Str(code.into())),
            ("ns".into(), B::Str("user".into())),
            ("id".into(), B::Str(id.clone())),
        ]);
        let mut bytes = Vec::new();
        encode(&msg, &mut bytes);
        self.out.write_all(&bytes).unwrap();
        let (mut value, mut err) = (None, String::new());
        loop {
            let first = byte(&mut self.input).expect("nREPL closed");
            let resp = decode_from(first, &mut self.input).expect("bad bencode");
            if resp.get("id").and_then(B::str) != Some(id.as_str()) {
                continue;
            }
            if let Some(v) = resp.get("value").and_then(B::str) {
                value = Some(v.to_owned());
            }
            for key in ["err", "ex"] {
                if let Some(e) = resp.get(key).and_then(B::str) {
                    err.push_str(e);
                }
            }
            let done = matches!(resp.get("status"), Some(B::List(s)) if s.iter().any(|x| x.str() == Some("done")));
            if done {
                break;
            }
        }
        match value {
            Some(v) if err.is_empty() => Ok(v),
            _ => Err(err),
        }
    }
}

fn live() -> Option<Repl> {
    let port = std::env::var("FOSFORA_IT_NREPL").ok()?.parse().ok()?;
    Some(Repl::connect(port))
}

#[test]
fn bencode_round_trips() {
    let v = B::Dict(vec![
        ("a".into(), B::List(vec![B::Int(-3), B::Str("x:y".into())])),
        ("b".into(), B::Str(String::new())),
    ]);
    let mut bytes = Vec::new();
    encode(&v, &mut bytes);
    let mut r = &bytes[..];
    let first = byte(&mut r).unwrap();
    assert_eq!(decode_from(first, &mut r).unwrap(), v);
}

#[test]
fn every_builtin_addon_is_reachable_over_nrepl() {
    let Some(mut repl) = live() else {
        eprintln!("skipped: FOSFORA_IT_NREPL not set");
        return;
    };
    let addons = repl.eval("(f/addons)").unwrap();
    for id in ["core", "preset", "scene", "layer", "particle", "trama", "bindings", "system"] {
        assert!(addons.contains(id), "{id} missing from {addons}");
    }
    let names = repl.eval("(mapv :name (f/ops))").unwrap();
    assert!(names.contains("preset/save"), "{names}");
    let e = repl.eval("(f/op :no/such-op)").unwrap_err();
    assert!(e.contains("unknown op"), "{e}");
}

#[test]
fn a_preset_saved_over_nrepl_lists_and_deletes() {
    let Some(mut repl) = live() else {
        eprintln!("skipped: FOSFORA_IT_NREPL not set");
        return;
    };
    let name = format!("it-repl-ops-{}", std::process::id());
    let before = repl.eval("(count (preset/ls))").unwrap();
    let saved = repl.eval(&format!("(preset/save! \"{name}\")"));
    // An empty stack cannot be saved; that is the app's answer, not a failure of the op path.
    if let Err(e) = &saved {
        assert!(e.contains("nothing to save"), "{e}");
        return;
    }
    let listed = repl.eval(&format!("(some #(= \"{name}\" (:name %)) (preset/ls))")).unwrap();
    assert_eq!(listed, "true");
    repl.eval(&format!("(preset/delete! \"{name}\")")).unwrap();
    assert_eq!(repl.eval("(count (preset/ls))").unwrap(), before);
}

#[test]
fn trama_chains_and_effects_read_over_nrepl() {
    let Some(mut repl) = live() else {
        eprintln!("skipped: FOSFORA_IT_NREPL not set");
        return;
    };
    // Reads only: the user's chains are left exactly as they are.
    let first = repl.eval("(:chain (first (trama/chains)))").unwrap();
    assert_eq!(first, "\"master\"");
    let output = repl.eval("(:output (trama/graph :chain :master))").unwrap();
    assert!(output.parse::<u64>().is_ok(), "{output}");
    let dirs = repl.eval("(count (:dirs (trama/effects)))").unwrap();
    assert_ne!(dirs, "0");
    let e = repl.eval("(trama/remove! \"output\" :chain :master)").unwrap_err();
    assert!(e.contains("Output"), "{e}");
}
