//! `molde database update` — aplica migraciones pendientes. Fase 1.

use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use molde_core::migration;
use molde_migrate::Migrator;
use molde_providers::Provider;

#[derive(Args)]
pub struct UpdateArgs {
    /// Cadena de conexión: URL sqlx (`postgres://…`, `mysql://…`, `sqlite://…`) o,
    /// para SQL Server, cadena ADO (`Server=…;Database=…;User Id=…;Password=…;TrustServerCertificate=true`).
    /// Por defecto toma la variable de entorno `DATABASE_URL`.
    #[arg(long, env = "DATABASE_URL")]
    pub connection: String,

    /// Motor: sqlite | postgres. Si se omite, se infiere de la URL.
    #[arg(long)]
    pub provider: Option<String>,

    /// Directorio de migraciones a aplicar.
    #[arg(long, default_value = "migrations")]
    pub migrations_dir: PathBuf,

    /// Lleva la BD hasta esta migración (id o nombre). `0` revierte todas.
    /// Por defecto, aplica todas las pendientes.
    #[arg(long)]
    pub target: Option<String>,
}

pub fn update(args: UpdateArgs) -> anyhow::Result<()> {
    let provider = resolve_provider(args.provider.as_deref(), &args.connection)?;

    let migrations = migration::load_dir(&args.migrations_dir)
        .with_context(|| format!("leyendo migraciones de {}", args.migrations_dir.display()))?;
    if migrations.is_empty() {
        tracing::warn!(
            "no se encontraron migraciones en {}",
            args.migrations_dir.display()
        );
    }

    // Solo este comando necesita async; aislamos el runtime aquí.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creando el runtime async")?;

    let report = runtime.block_on(async {
        let migrator = Migrator::connect(&args.connection, provider)
            .await
            .context("conectando a la base de datos")?;
        migrator
            .update(&migrations, args.target.as_deref())
            .await
            .context("aplicando migraciones")
    })?;

    if report.is_empty() {
        println!("La base de datos ya está actualizada. Nada que aplicar.");
    } else {
        for id in &report.reverted {
            println!("  ⤺ revertida  {id}");
        }
        for id in &report.applied {
            println!("  ✔ aplicada   {id}");
        }
        println!(
            "Listo: {} aplicada(s), {} revertida(s).",
            report.applied.len(),
            report.reverted.len()
        );
    }

    Ok(())
}

/// Determina el provider: explícito (`--provider`) o inferido de la URL.
fn resolve_provider(explicit: Option<&str>, url: &str) -> anyhow::Result<Provider> {
    match explicit {
        Some(p) => Provider::parse(p)
            .with_context(|| format!("provider no soportado: '{p}' (usa sqlite | postgres)")),
        None => Provider::from_url(url)
            .context("no se pudo inferir el provider desde la URL; especifícalo con --provider"),
    }
}
