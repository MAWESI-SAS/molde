//! `efrust scaffold` — database-first (BD → C#). Fase 2.

use std::path::PathBuf;

use anyhow::Context;
use clap::Args;
use efrust_providers::Provider;
use efrust_scaffold::CodegenOptions;

#[derive(Args)]
pub struct ScaffoldArgs {
    /// Cadena de conexión sqlx a la base de datos de origen.
    /// Por defecto toma la variable de entorno `DATABASE_URL`.
    #[arg(long, env = "DATABASE_URL")]
    pub connection: String,

    /// Motor: sqlite | postgres. Si se omite, se infiere de la URL.
    #[arg(long)]
    pub provider: Option<String>,

    /// Esquema a leer (solo Postgres). Por defecto `public`.
    #[arg(long)]
    pub schema: Option<String>,

    /// Directorio de salida para los modelos C# generados.
    #[arg(long, default_value = "Models")]
    pub output_dir: PathBuf,

    /// Namespace para las clases generadas.
    #[arg(long, default_value = "App.Data")]
    pub namespace: String,

    /// Nombre de la clase del DbContext a generar.
    #[arg(long, default_value = "AppDbContext")]
    pub context: String,

    /// Sobrescribe archivos existentes en el directorio de salida.
    #[arg(long)]
    pub force: bool,
}

pub fn run(args: ScaffoldArgs) -> anyhow::Result<()> {
    let provider = match args.provider.as_deref() {
        Some(p) => Provider::parse(p)
            .with_context(|| format!("provider no soportado: '{p}' (usa sqlite | postgres)"))?,
        None => Provider::from_url(&args.connection)
            .context("no se pudo inferir el provider desde la URL; usa --provider")?,
    };

    let opts = CodegenOptions {
        namespace: args.namespace,
        context_name: args.context,
    };

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creando el runtime async")?;

    let files = runtime.block_on(async {
        efrust_scaffold::build_files(&args.connection, provider, args.schema.as_deref(), &opts)
            .await
            .context("leyendo el esquema de la base de datos")
    })?;

    if files.is_empty() {
        println!("No se encontraron tablas para generar.");
        return Ok(());
    }

    std::fs::create_dir_all(&args.output_dir)
        .with_context(|| format!("creando el directorio {}", args.output_dir.display()))?;

    let mut written = 0;
    for file in &files {
        let path = args.output_dir.join(&file.relative_path);
        if path.exists() && !args.force {
            tracing::warn!("omitido (ya existe, usa --force): {}", path.display());
            continue;
        }
        std::fs::write(&path, &file.contents)
            .with_context(|| format!("escribiendo {}", path.display()))?;
        println!("  ✔ {}", path.display());
        written += 1;
    }

    println!(
        "Listo: {written} archivo(s) generado(s) en {}.",
        args.output_dir.display()
    );
    Ok(())
}
