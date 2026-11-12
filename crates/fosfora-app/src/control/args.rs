//! Argument helpers for op bodies. Args arrive as a JSON object; keywords
//! from the REPL become plain string keys (`:name` is `"name"`).

use serde_json::Value;

pub fn str<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing string arg :{key}"))
}

pub fn opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub fn usize(args: &Value, key: &str) -> Result<usize, String> {
    opt_usize(args, key).ok_or_else(|| format!("missing non-negative integer arg :{key}"))
}

pub fn opt_usize(args: &Value, key: &str) -> Option<usize> {
    args.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
}

pub fn f32(args: &Value, key: &str) -> Result<f32, String> {
    opt_f32(args, key).ok_or_else(|| format!("missing number arg :{key}"))
}

#[allow(clippy::cast_possible_truncation)]
pub fn opt_f32(args: &Value, key: &str) -> Option<f32> {
    args.get(key).and_then(Value::as_f64).map(|v| v as f32)
}

pub fn bool(args: &Value, key: &str) -> Result<bool, String> {
    opt_bool(args, key).ok_or_else(|| format!("missing boolean arg :{key}"))
}

pub fn opt_bool(args: &Value, key: &str) -> Option<bool> {
    args.get(key).and_then(Value::as_bool)
}

/// The ports the Setup page offers for a listener or a target.
pub const PORT_RANGE: std::ops::RangeInclusive<u16> = 1024..=65535;

/// A network port at `key` inside [`PORT_RANGE`]; absent is `None`.
pub fn opt_port(args: &Value, key: &str) -> Result<Option<u16>, String> {
    match args.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| PORT_RANGE.contains(n))
            .map(Some)
            .ok_or_else(|| {
                format!(
                    ":{key} must be a port in {}..={}, got {v}",
                    PORT_RANGE.start(),
                    PORT_RANGE.end()
                )
            }),
    }
}

/// A transition kind by name: `cut`, `dissolve`, `morph` (or `param_morph`),
/// any case. Absent is `None`; an unknown name is an error.
pub fn opt_transition(
    args: &Value,
    key: &str,
) -> Result<Option<crate::scene::types::TransitionType>, String> {
    use crate::scene::types::TransitionType as T;
    let Some(raw) = opt_str(args, key) else {
        return Ok(None);
    };
    match raw.to_ascii_lowercase().replace(['-', ' '], "_").as_str() {
        "cut" => Ok(Some(T::Cut)),
        "dissolve" => Ok(Some(T::Dissolve)),
        "morph" | "param_morph" | "parammorph" => Ok(Some(T::ParamMorph)),
        _ => Err(format!(
            ":{key} must be cut, dissolve or morph, got {raw:?}"
        )),
    }
}

/// The name [`opt_transition`] reads back.
pub fn transition_name(kind: crate::scene::types::TransitionType) -> &'static str {
    use crate::scene::types::TransitionType as T;
    match kind {
        T::Cut => "cut",
        T::Dissolve => "dissolve",
        T::ParamMorph => "morph",
    }
}

/// Resolve `{:index n}` or `{:name "x"}` against `names` (exact match first,
/// then a unique case-insensitive substring). Most "pick one" ops take either.
pub fn pick<S: AsRef<str>>(args: &Value, names: &[S], what: &str) -> Result<usize, String> {
    if let Some(i) = opt_usize(args, "index") {
        return if i < names.len() {
            Ok(i)
        } else {
            Err(format!(
                "{what} index {i} out of range (have {})",
                names.len()
            ))
        };
    }
    let name = str(args, "name").map_err(|_| format!("give {what} as :index or :name"))?;
    if let Some(i) = names.iter().position(|n| n.as_ref() == name) {
        return Ok(i);
    }
    let needle = name.to_lowercase();
    let hits: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_, n)| n.as_ref().to_lowercase().contains(&needle))
        .map(|(i, _)| i)
        .collect();
    match hits.as_slice() {
        [i] => Ok(*i),
        [] => Err(format!("no {what} matches {name:?}")),
        many => Err(format!(
            "{name:?} matches several {what}s: {:?}",
            many.iter().map(|&i| names[i].as_ref()).collect::<Vec<_>>()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pick_takes_an_index_an_exact_name_or_a_unique_substring() {
        let names = ["Tunnel", "Tessera Tunnel", "Dope"];
        assert_eq!(pick(&json!({"index": 2}), &names, "preset"), Ok(2));
        assert_eq!(pick(&json!({"name": "Tunnel"}), &names, "preset"), Ok(0));
        assert_eq!(pick(&json!({"name": "dop"}), &names, "preset"), Ok(2));
        assert!(pick(&json!({"name": "tun"}), &names, "preset").is_err());
        assert!(pick(&json!({"index": 9}), &names, "preset").is_err());
        assert!(pick(&json!({}), &names, "preset").is_err());
    }

    #[test]
    fn transition_names_round_trip_and_unknown_ones_are_refused() {
        for &kind in crate::scene::types::TransitionType::ALL {
            let a = json!({"k": transition_name(kind)});
            assert_eq!(opt_transition(&a, "k"), Ok(Some(kind)));
        }
        assert_eq!(
            opt_transition(&json!({"k": "Param-Morph"}), "k")
                .unwrap()
                .map(transition_name),
            Some("morph")
        );
        assert_eq!(opt_transition(&json!({}), "k"), Ok(None));
        assert!(opt_transition(&json!({"k": "wipe"}), "k").is_err());
    }
}
