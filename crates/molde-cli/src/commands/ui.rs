//! CLI UX layer: interactive prompts, spinner, and colored output.
//! Everything degrades gracefully without a terminal (CI/pipe): `console`
//! disables color, `indicatif` does not animate, and prompts are skipped
//! (the corresponding flag becomes required instead).

use std::io::IsTerminal;
use std::time::Duration;

use anyhow::{bail, Result};
use console::style;
use dialoguer::{Confirm, Input, Password};
use indicatif::{ProgressBar, ProgressStyle};

/// Can we ask questions? Only with a terminal and without `--no-input`.
pub fn interactive(no_input: bool) -> bool {
    !no_input && std::io::stdin().is_terminal()
}

/// Ask for a (required, non-empty) text value.
pub fn ask(prompt: &str) -> Result<String> {
    Ok(Input::new().with_prompt(prompt).interact_text()?)
}

/// Ask for a text value with a default.
#[allow(dead_code)]
pub fn ask_default(prompt: &str, default: &str) -> Result<String> {
    Ok(Input::new()
        .with_prompt(prompt)
        .default(default.to_string())
        .interact_text()?)
}

/// Ask for a password (hidden input).
#[allow(dead_code)]
pub fn ask_password(prompt: &str) -> Result<String> {
    Ok(Password::new().with_prompt(prompt).interact()?)
}

/// Ask for confirmation (yes/no) with a default.
pub fn confirm(prompt: &str, default: bool) -> Result<bool> {
    Ok(Confirm::new()
        .with_prompt(prompt)
        .default(default)
        .interact()?)
}

/// Create an animated spinner with a message.
pub fn spinner(msg: impl Into<String>) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap_or_else(|_| ProgressStyle::default_spinner()),
    );
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_message(msg.into());
    pb
}

pub fn ok(msg: impl AsRef<str>) {
    println!("{} {}", style("✔").green().bold(), msg.as_ref());
}

pub fn info(msg: impl AsRef<str>) {
    println!("{} {}", style("›").cyan().bold(), msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    eprintln!("{} {}", style("!").yellow().bold(), msg.as_ref());
}

pub fn header(msg: impl AsRef<str>) {
    println!("\n{}", style(msg.as_ref()).bold());
}

/// Resolve the connection string: flag/env → if missing and interactive, ask
/// for it → otherwise a clear error.
pub fn resolve_connection(conn: Option<String>, no_input: bool) -> Result<String> {
    if let Some(c) = conn.filter(|s| !s.trim().is_empty()) {
        return Ok(c);
    }
    if interactive(no_input) {
        let c = ask("Connection string (postgres://… or sqlite:…)")?;
        if c.trim().is_empty() {
            bail!("the connection cannot be empty");
        }
        Ok(c)
    } else {
        bail!("missing connection: pass --connection or set DATABASE_URL")
    }
}

/// Human-readable label for a connection, WITHOUT exposing the password.
pub fn redact(conn: &str) -> String {
    // postgres://user:pass@host:port/db?... → postgres://user:***@host/db
    if let Some((scheme, rest)) = conn.split_once("://") {
        if let Some((creds, tail)) = rest.split_once('@') {
            let user = creds.split(':').next().unwrap_or(creds);
            let hostdb = tail.split('?').next().unwrap_or(tail);
            return format!("{scheme}://{user}:***@{hostdb}");
        }
        let hostdb = rest.split('?').next().unwrap_or(rest);
        return format!("{scheme}://{hostdb}");
    }
    conn.to_string()
}

/// Database name extracted from a connection string (for messages).
pub fn db_name(conn: &str) -> String {
    conn.rsplit('/')
        .next()
        .map(|s| s.split('?').next().unwrap_or(s))
        .filter(|s| !s.is_empty())
        .unwrap_or(conn)
        .to_string()
}
