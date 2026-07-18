//! Standalone CopyQ clipboard history frontend entrypoint.

mod copyq_frontend;

use clap::Parser;
use copyq_frontend::{
    CopyqClient, CopyqFrontendOptions, apply_i3_shortcut_defaults, i3_config_snippet,
};

#[derive(Debug, Parser)]
#[command(name = "deskhalloumi-copyq")]
#[command(about = "Fast, keyboard-friendly CopyQ clipboard history frontend")]
struct Args {
    /// Path to the CopyQ executable.
    #[arg(long, default_value = "copyq", value_name = "PATH")]
    copyq: String,

    /// Maximum preview length per clipboard item.
    #[arg(long, default_value_t = 220, value_name = "CHARS")]
    max_preview_chars: usize,

    /// Maximum rows rendered at once after filtering.
    #[arg(long, default_value_t = 160, value_name = "ROWS")]
    max_visible_rows: usize,

    /// Start with i3-friendly popup defaults: borderless, centered, always-on-top, close on focus loss.
    #[arg(long)]
    i3_shortcut: bool,

    /// Close the popup when it loses focus.
    #[arg(long)]
    close_on_unfocus: bool,

    /// Popup width in logical pixels.
    #[arg(long, default_value_t = 880, value_name = "PX")]
    width: u32,

    /// Popup height in logical pixels.
    #[arg(long, default_value_t = 640, value_name = "PX")]
    height: u32,

    /// Stable legacy Linux application id used by existing i3 rules.
    #[arg(long, default_value = "unilii-copyq", value_name = "ID")]
    app_id: String,

    /// Window title, useful for WM rules and debugging.
    #[arg(long, default_value = "DeskHalloumi CopyQ", value_name = "TITLE")]
    title: String,

    /// Print an example i3 config snippet and exit.
    #[arg(long, value_name = "EXECUTABLE")]
    print_i3_config: Option<String>,

    /// Modifier used in the printed i3 config snippet.
    #[arg(long, default_value = "$mod", value_name = "MOD")]
    i3_modifier: String,

    /// Only select the item in CopyQ; do not paste into the focused application.
    #[arg(long)]
    no_paste: bool,

    /// Print history as normalized JSON and exit. Useful for launcher integrations and tests.
    #[arg(long)]
    print_json: bool,
}

fn main() -> iced::Result {
    let _menu_instance =
        match deskhalloumi_core::menu_process::MenuProcessManager::register_current_process("copyq")
        {
            Ok(guard) => guard,
            Err(error) => {
                eprintln!("{error}");
                return Ok(());
            }
        };

    let args = Args::parse();
    let _ = tracing_subscriber::fmt().try_init();

    if let Some(executable) = args.print_i3_config.as_deref() {
        println!("{}", i3_config_snippet(executable, &args.i3_modifier));
        return Ok(());
    }

    let options = CopyqFrontendOptions {
        copyq_bin: args.copyq,
        max_preview_chars: args.max_preview_chars,
        max_visible_rows: args.max_visible_rows,
        paste_on_activate: !args.no_paste,
        window_width: args.width,
        window_height: args.height,
        i3_shortcut_mode: args.i3_shortcut,
        close_on_unfocus: args.close_on_unfocus,
        window_title: args.title,
        application_id: args.app_id,
    };
    let options = if args.i3_shortcut {
        apply_i3_shortcut_defaults(options)
    } else {
        options
    };

    if args.print_json {
        let runtime = tokio::runtime::Runtime::new().map_err(|error| {
            iced::Error::WindowCreationFailed(
                format!("failed to create CopyQ command runtime: {error}").into(),
            )
        })?;
        match runtime.block_on(CopyqClient::new(&options).list_items()) {
            Ok(items) => println!(
                "{}",
                serde_json::to_string_pretty(&items).unwrap_or_else(|_| "[]".into())
            ),
            Err(error) => {
                eprintln!("{}", error);
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    copyq_frontend::run(options)
}
