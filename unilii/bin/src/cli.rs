//! Command-line interface for the DeskHalloumi desktop bar.
//!
//! This module defines the CLI structure using clap, providing a clean and
//! intuitive interface for running and configuring DeskHalloumi.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// DeskHalloumi: a modern, modular desktop bar for Linux
///
/// DeskHalloumi provides a feature-rich status bar with support for modules,
/// global keybindings, system tray integration, and configurable themes.
#[derive(Parser, Debug, Clone)]
#[command(name = "deskhalloumi")]
#[command(author = "DeskHalloumi contributors")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "A modular desktop bar and control surface for Linux", long_about = None)]
#[derive(Default)]
pub struct Cli {
    /// Path to configuration file (default: ~/.config/deskhalloumi/deskhalloumi.toml)
    #[arg(long, short = 'c', value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Verbose mode (-v, -vv, -vvv, etc.)
    #[arg(short, long, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Subcommands for advanced operations
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// Subcommands for DeskHalloumi
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Run the status bar (default command)
    Run {
        /// Disable tray icon support
        #[arg(long)]
        no_tray: bool,

        /// Disable network menu (requires nmcli)
        #[arg(long)]
        no_network_menu: bool,

        /// Path to nmcli binary (default: nmcli)
        #[arg(long, default_value = "nmcli", value_name = "PATH")]
        nmcli_path: String,

        /// Tray icon polling interval in milliseconds (default: 1500)
        #[arg(long, default_value_t = 1500, value_name = "MS")]
        tray_poll_ms: u64,

        /// Enable debug focus mode (show window decorations, allow resizing)
        #[arg(long)]
        debug_focus: bool,

        /// Do not start the bar-embedded hotkey listener (use standalone deskhalloumi-hotkeyd).
        #[arg(long)]
        no_hotkeyd: bool,
    },

    /// List available modules
    ListModules,

    /// Show current configuration
    ShowConfig,

    /// Generate a default configuration file
    InitConfig {
        /// Output file path (default: ~/.config/deskhalloumi/deskhalloumi.toml)
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,

        /// Force overwrite existing configuration
        #[arg(long)]
        force: bool,
    },

    /// Validate current configuration
    ValidateConfig {
        /// Configuration file to validate (default: ~/.config/com/unilii/unilii.toml)
        #[arg(short, long, value_name = "FILE")]
        config: Option<PathBuf>,
    },

    /// Generate a starter configuration for the headless unilii-bar scaffold
    InitBarConfig {
        /// Output file path. If omitted, print to stdout.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,

        /// Force overwrite existing configuration
        #[arg(long)]
        force: bool,
    },

    /// Validate a unilii-bar configuration file
    ValidateBarConfig {
        /// Bar configuration file to validate
        #[arg(short, long, value_name = "FILE")]
        config: Option<PathBuf>,
    },

    /// Display version information
    Version,

    /// Simulate key events against loaded bindings and print trigger diagnostics
    KeyDryRun {
        /// Configuration file to load keybindings from
        #[arg(short, long, value_name = "FILE")]
        config: Option<PathBuf>,

        /// Optional sxhkd config file to import instead of TOML keybindings
        #[arg(long, value_name = "FILE")]
        sxhkd: Option<PathBuf>,

        /// Comma-separated events, e.g. KEY_LEFTMETA:1,KEY_RETURN:1,KEY_RETURN:0,KEY_LEFTMETA:0
        #[arg(long, value_name = "EVENTS")]
        events: String,
    },
}

impl Commands {
    /// Check if this is the default run command
    #[allow(dead_code)]
    pub fn is_run(&self) -> bool {
        matches!(self, Commands::Run { .. })
    }

    /// Get run command options if present
    pub fn run_options(&self) -> Option<RunOptions> {
        match self {
            Commands::Run {
                no_tray,
                no_network_menu,
                nmcli_path,
                tray_poll_ms,
                debug_focus,
                no_hotkeyd,
            } => Some(RunOptions {
                no_tray: *no_tray,
                no_network_menu: *no_network_menu,
                nmcli_path: nmcli_path.clone(),
                tray_poll_ms: *tray_poll_ms,
                debug_focus: *debug_focus,
                no_hotkeyd: *no_hotkeyd,
            }),
            _ => None,
        }
    }
}

/// Options for running the status bar
#[derive(Debug, Clone)]
pub struct RunOptions {
    #[allow(dead_code)]
    pub no_tray: bool,
    #[allow(dead_code)]
    pub no_network_menu: bool,
    #[allow(dead_code)]
    pub nmcli_path: String,
    #[allow(dead_code)]
    pub tray_poll_ms: u64,
    pub debug_focus: bool,
    pub no_hotkeyd: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            no_tray: false,
            no_network_menu: false,
            nmcli_path: "nmcli".to_string(),
            tray_poll_ms: 1500,
            debug_focus: false,
            no_hotkeyd: false,
        }
    }
}

/// Parse verbose flag into tracing level
pub fn verbose_to_level(verbose: u8) -> tracing::Level {
    match verbose {
        0 => tracing::Level::INFO,
        1 => tracing::Level::DEBUG,
        2..=3 => tracing::Level::TRACE,
        _ => tracing::Level::TRACE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_cli_works() {
        let cli = Cli::default();
        assert!(cli.config.is_none());
        assert_eq!(cli.verbose, 0);
        assert!(cli.command.is_none());
    }

    #[test]
    fn run_parses_no_hotkeyd_handoff() {
        let cli = Cli::try_parse_from(["unilii", "run", "--no-hotkeyd"]).expect("CLI should parse");
        let options = cli.command.unwrap().run_options().unwrap();
        assert!(options.no_hotkeyd);
    }

    #[test]
    fn verbose_levels() {
        assert_eq!(verbose_to_level(0), tracing::Level::INFO);
        assert_eq!(verbose_to_level(1), tracing::Level::DEBUG);
        assert_eq!(verbose_to_level(2), tracing::Level::TRACE);
        assert_eq!(verbose_to_level(10), tracing::Level::TRACE);
    }

    #[test]
    fn run_options_default() {
        let opts = RunOptions::default();
        assert!(!opts.no_tray);
        assert!(!opts.no_network_menu);
        assert_eq!(opts.nmcli_path, "nmcli");
        assert_eq!(opts.tray_poll_ms, 1500);
        assert!(!opts.debug_focus);
        assert!(!opts.no_hotkeyd);
    }
}
