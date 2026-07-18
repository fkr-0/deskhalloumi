use std::env;
use std::os::unix::process::CommandExt;
use std::process::{Command, exit};

pub fn exec_sibling(primary_name: &str) -> ! {
    let current = env::current_exe().unwrap_or_else(|error| {
        eprintln!("legacy DeskHalloumi alias could not resolve itself: {error}");
        exit(127);
    });
    let primary = current.with_file_name(primary_name);
    let error = Command::new(&primary).args(env::args_os().skip(1)).exec();
    eprintln!(
        "legacy DeskHalloumi alias could not execute '{}': {error}",
        primary.display()
    );
    exit(127);
}
