use deskhalloumi_core::bar::{
    BarConfig, load_bar_config, load_default_or_starter_bar_config, starter_bar_config_toml,
};
use deskhalloumi_core::bar_runtime::{
    BarModuleGraph, BarModuleViewModel, BarReloadStatus, BarRuntimeContext, BarRuntimeState,
};
use std::env;
use std::path::PathBuf;
use std::process;
use std::thread;
use std::time::Duration;

#[derive(Debug, PartialEq, Eq)]
struct BarCli {
    config: Option<PathBuf>,
    print_default_config: bool,
    check_config: bool,
    watch: bool,
    ticks: Option<u64>,
    tick_interval_ms: u64,
    help: bool,
}

impl Default for BarCli {
    fn default() -> Self {
        Self {
            config: None,
            print_default_config: false,
            check_config: false,
            watch: false,
            ticks: None,
            tick_interval_ms: 1000,
            help: false,
        }
    }
}

fn main() {
    match run(env::args().skip(1)) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("unilii-bar: {err}");
            process::exit(2);
        }
    }
}

fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let cli = parse_args(args)?;
    if cli.help {
        print_help();
        return Ok(());
    }

    if cli.print_default_config {
        print!("{}", starter_bar_config_toml());
        return Ok(());
    }

    let config = match &cli.config {
        Some(path) => load_bar_config(path).map_err(|err| err.to_string())?,
        None => load_default_or_starter_bar_config().map_err(|err| err.to_string())?,
    };

    if cli.check_config {
        println!(
            "bar config ok: {} modules, height={}px, position={:?}",
            config.modules.len(),
            config.bar.height,
            config.bar.position
        );
        return Ok(());
    }

    if cli.watch {
        let state = load_runtime_state(&cli, config)?;
        run_watch_loop(
            state,
            cli.ticks,
            Duration::from_millis(cli.tick_interval_ms),
        )
    } else {
        print_headless_summary(&config);
        let mut graph = BarModuleGraph::from_config(&config).map_err(|err| err.to_string())?;
        let render_model = graph.update_render_model(&BarRuntimeContext::default());
        print_zone("left", &render_model.left);
        print_zone("center", &render_model.center);
        print_zone("right", &render_model.right);
        Ok(())
    }
}

fn load_runtime_state(cli: &BarCli, config: BarConfig) -> Result<BarRuntimeState, String> {
    match &cli.config {
        Some(path) => BarRuntimeState::from_config_file(path).map_err(|err| err.to_string()),
        None => BarRuntimeState::from_config(config).map_err(|err| err.to_string()),
    }
}

fn run_watch_loop(
    mut state: BarRuntimeState,
    ticks: Option<u64>,
    tick_interval: Duration,
) -> Result<(), String> {
    let mut tick = 0_u64;
    loop {
        tick += 1;
        match state.reload_from_file_if_changed() {
            Ok(BarReloadStatus::Reloaded) => println!("reload: reloaded"),
            Ok(BarReloadStatus::Failed) => {
                println!(
                    "reload: failed: {}",
                    state.last_reload_error().unwrap_or("unknown reload error")
                );
            }
            Ok(BarReloadStatus::Unchanged) => {}
            Err(error) => println!("reload: failed: {error}"),
        }

        let render_model = state
            .graph_mut()
            .update_due_render_model(&BarRuntimeContext::default());
        println!("tick {tick}");
        print_zone("left", &render_model.left);
        print_zone("center", &render_model.center);
        print_zone("right", &render_model.right);

        if ticks.is_some_and(|limit| tick >= limit) {
            return Ok(());
        }

        let scheduler_wait = state
            .graph_mut()
            .next_due_in(std::time::SystemTime::now())
            .unwrap_or(tick_interval);
        let sleep_for = scheduler_wait.min(tick_interval);
        if sleep_for > Duration::ZERO {
            thread::sleep(sleep_for);
        }
    }
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<BarCli, String> {
    let mut cli = BarCli::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => cli.help = true,
            "--print-default-config" | "generate-config" => cli.print_default_config = true,
            "--check-config" => cli.check_config = true,
            "--watch" => cli.watch = true,
            "--ticks" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{arg} requires a numeric argument"))?;
                cli.ticks = Some(
                    value
                        .parse()
                        .map_err(|_| format!("{arg} requires a positive integer"))?,
                );
            }
            "--tick-interval-ms" => {
                let value = args
                    .next()
                    .ok_or_else(|| format!("{arg} requires a numeric argument"))?;
                cli.tick_interval_ms = value
                    .parse()
                    .map_err(|_| format!("{arg} requires a non-negative integer"))?;
            }
            "-c" | "--config" => {
                let path = args
                    .next()
                    .ok_or_else(|| format!("{arg} requires a path argument"))?;
                cli.config = Some(PathBuf::from(path));
            }
            unknown => return Err(format!("unknown argument '{unknown}'")),
        }
    }
    Ok(cli)
}

fn print_zone(name: &str, models: &[BarModuleViewModel]) {
    println!("zone {name}:");
    if models.is_empty() {
        println!("  <empty>");
        return;
    }
    for model in models {
        println!(
            "  module {} [{}]: {} ({:?})",
            model.id, model.module_type, model.label, model.state
        );
    }
}

fn print_help() {
    println!(
        "unilii-bar\n\nUSAGE:\n  unilii-bar [--config <path>] [--check-config]\n  unilii-bar --print-default-config\n\nOPTIONS:\n  -c, --config <path>       Load a TOML bar config\n      --check-config        Validate config and exit\n      --print-default-config Print starter TOML config\n  -h, --help                Print this help"
    );
}

fn print_headless_summary(config: &BarConfig) {
    println!("unilii-bar scaffold");
    println!("height: {}px", config.bar.height);
    println!("position: {:?}", config.bar.position);
    println!("modules: {}", config.modules.len());
    println!("left: {}", config.layout.left.join(", "));
    println!("center: {}", config.layout.center.join(", "));
    println!("right: {}", config.layout.right.join(", "));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_config_flag() {
        let cli = parse_args(["--print-default-config".to_string()]).unwrap();
        assert!(cli.print_default_config);
    }

    #[test]
    fn parses_config_and_check_flags() {
        let cli = parse_args([
            "--config".to_string(),
            "templates/bar.toml".to_string(),
            "--check-config".to_string(),
        ])
        .unwrap();
        assert_eq!(cli.config, Some(PathBuf::from("templates/bar.toml")));
        assert!(cli.check_config);
    }

    #[test]
    fn parses_watch_loop_flags() {
        let cli = parse_args([
            "--watch".to_string(),
            "--ticks".to_string(),
            "3".to_string(),
            "--tick-interval-ms".to_string(),
            "0".to_string(),
        ])
        .unwrap();
        assert!(cli.watch);
        assert_eq!(cli.ticks, Some(3));
        assert_eq!(cli.tick_interval_ms, 0);
    }

    #[test]
    fn rejects_missing_config_path() {
        let err = parse_args(["--config".to_string()]).unwrap_err();
        assert!(err.contains("requires a path argument"));
    }

    #[test]
    fn rejects_missing_ticks_argument() {
        let err = parse_args(["--ticks".to_string()]).unwrap_err();
        assert!(err.contains("requires a numeric argument"));
    }
}
