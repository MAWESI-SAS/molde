//! `efrust scaffold` — database-first. Emite archivos `.model` (lenguaje EFM)
//! por defecto, o C# legacy con `--format csharp`.

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

    /// Formato de salida: `model` (lenguaje EFM, por defecto) | `csharp` (legacy).
    #[arg(long, default_value = "model")]
    pub format: String,

    /// Directorio de salida. Por defecto `models` (EFM) o `Models` (C#).
    #[arg(long)]
    pub output_dir: Option<PathBuf>,

    /// Namespace para las clases C# (solo `--format csharp`).
    #[arg(long, default_value = "App.Data")]
    pub namespace: String,

    /// Nombre de la clase del DbContext C# (solo `--format csharp`).
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

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("creando el runtime async")?;

    // (nombre_relativo, contenido) independiente del formato.
    let (files, default_dir): (Vec<(String, String)>, &str) = match args.format.as_str() {
        "model" | "efm" => {
            let mf = runtime.block_on(async {
                efrust_scaffold::build_model_files(
                    &args.connection,
                    provider,
                    args.schema.as_deref(),
                )
                .await
                .context("leyendo el esquema de la base de datos")
            })?;
            (
                mf.into_iter().map(|f| (f.name, f.contents)).collect(),
                "models",
            )
        }
        "csharp" | "cs" => {
            let opts = CodegenOptions {
                namespace: args.namespace.clone(),
                context_name: args.context.clone(),
                provider,
            };
            let gf = runtime.block_on(async {
                efrust_scaffold::build_files(
                    &args.connection,
                    provider,
                    args.schema.as_deref(),
                    &opts,
                )
                .await
                .context("leyendo el esquema de la base de datos")
            })?;
            (
                gf.into_iter()
                    .map(|f| (f.relative_path, f.contents))
                    .collect(),
                "Models",
            )
        }
        other => {
            anyhow::bail!("formato no soportado: '{other}' (usa model | csharp)");
        }
    };

    if files.is_empty() {
        println!("No se encontraron tablas para generar.");
        return Ok(());
    }

    let output_dir = args
        .output_dir
        .unwrap_or_else(|| PathBuf::from(default_dir));
    std::fs::create_dir_all(&output_dir)
        .with_context(|| format!("creando el directorio {}", output_dir.display()))?;

    let mut written = 0;
    for (name, contents) in &files {
        let path = output_dir.join(name);
        if path.exists() && !args.force {
            tracing::warn!("omitido (ya existe, usa --force): {}", path.display());
            continue;
        }
        std::fs::write(&path, contents)
            .with_context(|| format!("escribiendo {}", path.display()))?;
        println!("  ✔ {}", path.display());
        written += 1;
    }

    println!(
        "Listo: {written} archivo(s) generado(s) en {}.",
        output_dir.display()
    );
    Ok(())
}
