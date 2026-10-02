mod actions;
mod activation;
mod application;
mod banner;
mod cli;
mod column;
mod compare;
mod config;
mod editor;
mod edits;
mod loader;
mod marks;
mod monitor;
mod page_search;
mod preferences;
mod search;
mod selftest;
mod session;
mod tabstops;
mod theme;
mod window;
mod worker;

use gtk4::glib::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    let started = Instant::now();
    if let Err(error) = stet_infrastructure::logging::init() {
        eprintln!("Could not initialize logging: {error}");
        return ExitCode::FAILURE;
    }
    // Parsed here first, so a typo never reaches a running editor (ADR-011).
    let parsed = match cli::parse(std::env::args_os()) {
        Ok(parsed) => parsed,
        Err(error) => {
            let _ = error.print();
            return ExitCode::from(error.exit_code() as u8);
        }
    };
    let exit_after = match config::exit_after_from_env() {
        Ok(exit_after) => exit_after,
        Err(message) => {
            tracing::error!("{message}");
            return ExitCode::FAILURE;
        }
    };
    if let Some(script) = parsed.self_test {
        return selftest::run(&script, started);
    }
    let config = config::AppConfig::from_env(exit_after);
    // A command line goes to a running Stet, started through D-Bus if need be, so that
    // `--wait` waits for its own tabs only (ADR-011). The service itself skips this.
    if !parsed.service {
        let outcome = activation::prepare(&config.app_id, parsed.wait);
        tracing::debug!(?outcome, "command line");
    }
    application::run(config, started)
}
