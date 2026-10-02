//! D-Bus activation for a command line (ADR-011). GApplication only waits for the primary
//! instance's answer when the command line is a *remote*: when no Stet runs, the process
//! that was given the command line would become the editor itself, and `stet --wait` would
//! block its caller until the whole editor quits. So before the application registers, a
//! command line makes sure that a primary exists: it asks the session bus to start the
//! service (`StartServiceByName`, served by the installed `.service` file), and then runs as
//! a remote of the instance that started.
//!
//! Without a service file (a development build) a `--wait` command line starts
//! `stet --gapplication-service` itself, detached in its own process group with the same
//! application id, and waits until it owns the name. Any other command line then simply
//! becomes the primary, as before M2; so does every command line when there is no session
//! bus or activation fails.
//!
//! Everything here runs before GTK starts, on the main thread, and blocks only that process.

use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const BUS_NAME: &str = "org.freedesktop.DBus";
const BUS_PATH: &str = "/org/freedesktop/DBus";
const SERVICE_UNKNOWN: &str = "org.freedesktop.DBus.Error.ServiceUnknown";

/// How long starting the primary may take before this process becomes the primary itself.
const START_TIMEOUT: Duration = Duration::from_secs(10);

/// How often the fallback looks whether the service it started owns the name yet.
const POLL: Duration = Duration::from_millis(10);

/// How the command line reaches a primary instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A Stet with this id runs already; GApplication forwards the command line to it.
    Running,
    /// The bus started the service from its `.service` file.
    Activated,
    /// No service file: this process started `stet --gapplication-service` itself.
    Spawned,
    /// No session bus, no service and no `--wait`, or activation failed: this process
    /// becomes the primary.
    Primary,
}

/// Makes sure a primary instance owns `app_id` when that is possible; see the module docs.
/// `wait` is the command line's `--wait`.
pub fn prepare(app_id: &str, wait: bool) -> Outcome {
    let connection = match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(connection) => connection,
        Err(error) => {
            tracing::info!(%error, "no session bus; this process is the editor");
            return Outcome::Primary;
        }
    };
    if has_owner(&connection, app_id) {
        return Outcome::Running;
    }
    let started = Instant::now();
    match start_service(&connection, app_id) {
        Ok(()) => {
            tracing::debug!(
                ms = started.elapsed().as_secs_f64() * 1e3,
                "the session bus started Stet"
            );
            Outcome::Activated
        }
        Err(error) if gio::DBusError::remote_error(&error).as_deref() == Some(SERVICE_UNKNOWN) => {
            if !wait {
                return Outcome::Primary;
            }
            match spawn_service(&connection, app_id) {
                Ok(()) => Outcome::Spawned,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "could not start Stet in the background; --wait waits for the whole \
                         editor"
                    );
                    Outcome::Primary
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "D-Bus activation failed; this process is the editor");
            Outcome::Primary
        }
    }
}

fn has_owner(connection: &gio::DBusConnection, name: &str) -> bool {
    connection
        .call_sync(
            Some(BUS_NAME),
            BUS_PATH,
            BUS_NAME,
            "NameHasOwner",
            Some(&(name,).to_variant()),
            Some(glib::VariantTy::new("(b)").expect("a valid type")),
            gio::DBusCallFlags::NONE,
            2000,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|reply| reply.get::<(bool,)>())
        .is_some_and(|(owned,)| owned)
}

/// `StartServiceByName`: the bus answers once the started service owns the name.
fn start_service(connection: &gio::DBusConnection, name: &str) -> Result<(), glib::Error> {
    connection
        .call_sync(
            Some(BUS_NAME),
            BUS_PATH,
            BUS_NAME,
            "StartServiceByName",
            Some(&(name, 0u32).to_variant()),
            Some(glib::VariantTy::new("(u)").expect("a valid type")),
            gio::DBusCallFlags::NONE,
            START_TIMEOUT.as_millis() as i32,
            gio::Cancellable::NONE,
        )
        .map(|_| ())
}

/// The development fallback: `stet --gapplication-service` from this executable, in its own
/// process group with no terminal input or output, then a wait until it owns the name.
fn spawn_service(connection: &gio::DBusConnection, name: &str) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let home = std::env::home_dir().unwrap_or_else(|| "/".into());
    let mut child = Command::new(exe)
        .arg("--gapplication-service")
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    while started.elapsed() < START_TIMEOUT {
        if has_owner(connection, name) {
            tracing::debug!(
                ms = started.elapsed().as_secs_f64() * 1e3,
                "started Stet in the background"
            );
            return Ok(());
        }
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("it exited with {status}"));
        }
        std::thread::sleep(POLL);
    }
    let _ = child.kill();
    Err("it did not register in time".to_owned())
}
