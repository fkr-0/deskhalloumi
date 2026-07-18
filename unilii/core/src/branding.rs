//! DeskHalloumi naming compatibility helpers.

use std::ffi::OsString;
use std::path::PathBuf;

pub const PRODUCT_NAME: &str = "DeskHalloumi";
pub const COMMAND_PREFIX: &str = "deskhalloumi";
pub const LEGACY_COMMAND_PREFIX: &str = "unilii";

pub fn preferred_env(new_name: &str, legacy_name: &str) -> Option<OsString> {
    std::env::var_os(new_name).or_else(|| std::env::var_os(legacy_name))
}

pub fn xdg_config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
}

pub fn config_dir() -> Option<PathBuf> {
    xdg_config_home().map(|root| root.join(COMMAND_PREFIX))
}

pub fn legacy_config_dir() -> Option<PathBuf> {
    xdg_config_home().map(|root| root.join(LEGACY_COMMAND_PREFIX))
}

pub fn config_file_with_fallback(filename: &str) -> Option<PathBuf> {
    let current = config_dir()?.join(filename);
    if current.exists() {
        return Some(current);
    }
    let legacy = legacy_config_dir()?.join(filename);
    if legacy.exists() {
        Some(legacy)
    } else {
        Some(current)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_environment_value_has_precedence() {
        unsafe {
            std::env::set_var("DESKHALLOUMI_TEST_PRECEDENCE", "new");
            std::env::set_var("UNILII_TEST_PRECEDENCE", "legacy");
        }
        assert_eq!(
            preferred_env("DESKHALLOUMI_TEST_PRECEDENCE", "UNILII_TEST_PRECEDENCE"),
            Some(OsString::from("new"))
        );
        unsafe {
            std::env::remove_var("DESKHALLOUMI_TEST_PRECEDENCE");
            std::env::remove_var("UNILII_TEST_PRECEDENCE");
        }
    }
}
