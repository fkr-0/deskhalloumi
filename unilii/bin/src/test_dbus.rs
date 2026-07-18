use std::env;

mod enhanced_tray;
mod menus;
mod tray;

use enhanced_tray::dbus::test_real_status_notifier_functionality;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== StatusNotifier DBus Integration Test ===");
    println!();

    if env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!("This program tests the DBus integration with real StatusNotifier applications.");
        println!("Make sure you have running applications with system tray support:");
        println!("  - Discord, Slack, Teams, Steam, Spotify");
        println!("  - NetworkManager applet (nm-applet)");
        println!("  - Any Qt or modern GTK app with tray support");
        println!();
        println!("Usage: cargo run --bin test_dbus");
        return;
    }

    match test_real_status_notifier_functionality().await {
        Ok(_) => {
            println!();
            println!("✅ DBus integration test completed successfully!");
            println!("The enhanced tray system can now parse real StatusNotifier menus.");
        }
        Err(e) => {
            eprintln!();
            eprintln!("❌ DBus integration test failed: {}", e);
            eprintln!("This might be due to:");
            eprintln!("  - No DBus session running");
            eprintln!("  - No StatusNotifier watcher available");
            eprintln!("  - No applications with tray support running");
            std::process::exit(1);
        }
    }
}
