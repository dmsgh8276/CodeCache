//! `codecache config [KEY [VALUE]]` handler (M7.3, D18).
//!
//! - No `KEY` ⇒ load and print the resolved config as TOML (the read path; the §7.2 "print the
//!   whole resolved config" behavior).
//! - `KEY VALUE` ⇒ load, set the documented scalar key, and persist via [`Config::save`] (D18,
//!   non-clobbering — the whole config is re-serialized).
//!
//! Supported dotted keys are matched explicitly; an unknown key or an unparseable value returns an
//! error (nonzero exit) rather than panicking. At least `storage.max_db_size_mb` (the §7.2 example)
//! is supported.

use std::path::Path;

use anyhow::{bail, Context, Result};

use super::paths;

/// Read (no key) or write (`key value`) a configuration setting.
pub fn run(key: Option<&str>, value: Option<&str>, _db_path: &Path) -> Result<()> {
    let root =
        std::env::current_dir().context("could not resolve the current working directory")?;

    match (key, value) {
        // Read: print the whole resolved config as TOML.
        (None, _) => {
            let config = paths::load_config(&root)?;
            let toml = toml::to_string(&config).context("could not serialize config to TOML")?;
            print!("{toml}");
            Ok(())
        }
        // Write: set the scalar key and persist.
        (Some(key), Some(value)) => {
            let mut config = paths::load_config(&root)?;
            set_key(&mut config, key, value)?;
            let path = paths::config_path(&root);
            config.save(&path).map_err(anyhow::Error::new)?;
            println!("Set {key} = {value}");
            Ok(())
        }
        // A key with no value: ambiguous for v0.1 (we do not support per-key read yet). Surface a
        // clear error rather than silently doing nothing.
        (Some(key), None) => {
            bail!("missing value for config key `{key}` (usage: codecache config <KEY> <VALUE>)")
        }
    }
}

/// Apply a `<KEY> <VALUE>` write to `config`. Only the documented scalar keys are routable; an
/// unknown key or an unparseable value is an error (no panic).
fn set_key(config: &mut crate::config::Config, key: &str, value: &str) -> Result<()> {
    match key {
        "storage.max_db_size_mb" => {
            config.storage.max_db_size_mb = parse(key, value)?;
        }
        other => bail!("unknown config key `{other}`"),
    }
    Ok(())
}

/// Parse a scalar config value, mapping a parse failure to a clear error (no panic).
fn parse<T>(key: &str, value: &str) -> Result<T>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|e| anyhow::anyhow!("invalid value `{value}` for config key `{key}`: {e}"))
}
