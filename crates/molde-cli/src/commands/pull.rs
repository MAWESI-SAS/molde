//! `molde pull` — database-first: introspect the database → `.model` files.

use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use molde_providers::Provider;

use crate::commands::ui;

#[derive(Args)]
pub struct PullArgs {
    /// Connection string to the source database.
    /// Defaults to the `DATABASE_URL` env var; if missing, you are prompted.
    #[arg(long, short = 'c', env = "DATABASE_URL")]
    pub connection: Option<String>,

    /// Engine: sqlite | postgres | mysql | sqlserver. Inferred from the URL if omitted.
    #[arg(long)]
    pub provider: Option<String>,

    /// Schema to read (Postgres only). Defaults to `public`.
    #[arg(long)]
    pub schema: Option<String>,

    /// Output directory for the `.model` files.
    #[arg(long, short = 'o', default_value = "models")]
    pub out: PathBuf,

    /// Overwrite existing files in the output directory.
    #[arg(long)]
    pub force: bool,

    /// Do not ask interactive questions (for CI). Fails if data is missing.
    #[arg(long)]
    pub no_input: bool,
}

pub fn run(args: PullArgs) -> anyhow::Result<()> {
    ui::header("molde pull · database → models");
    let connection = ui::resolve_connection(args.connection, args.no_input)?;

    let provider = match args.provider.as_deref() {
        Some(p) => Provider::parse(p).with_context(|| {
            format!("unsupported provider: '{p}' (use sqlite | postgres | mysql | sqlserver)")
        })?,
        None => Provider::from_url(&connection)
            .context("could not infer the provider from the URL; use --provider")?,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creating the async runtime")?;

    let sp = ui::spinner(format!("reading the schema of {}", ui::redact(&connection)));
    let files = runtime.block_on(async {
        molde_scaffold::build_model_files(&connection, provider, args.schema.as_deref())
            .await
            .context("reading the database schema")
    });
    sp.finish_and_clear();
    let files = files?;

    if files.is_empty() {
        ui::warn("no tables found to generate.");
        return Ok(());
    }

    std::fs::create_dir_all(&args.out)
        .with_context(|| format!("creating the directory {}", args.out.display()))?;

    let mut written = 0;
    let mut skipped = 0;
    for file in &files {
        let path = args.out.join(&file.name);
        if path.exists() && !args.force {
            skipped += 1;
            continue;
        }
        std::fs::write(&path, &file.contents)
            .with_context(|| format!("writing {}", path.display()))?;
        written += 1;
    }

    ui::ok(format!(
        "{written} .model file(s) in {}/",
        args.out.display()
    ));
    if skipped > 0 {
        ui::warn(format!(
            "{skipped} skipped (already exist; use --force to overwrite)"
        ));
    }
    Ok(())
}
