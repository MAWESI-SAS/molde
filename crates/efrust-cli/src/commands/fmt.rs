//! `efrust fmt` — formatea archivos `.model` a su forma canónica.
//!
//! Tres modos:
//! - rutas (archivos o directorios): formatea in situ, o solo verifica con `--check`.
//! - `--stdin`: lee de stdin y escribe el resultado en stdout (para editores).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;

#[derive(Args)]
pub struct FmtArgs {
    /// Archivos `.model` o directorios a formatear. Por defecto `models/`.
    pub paths: Vec<PathBuf>,

    /// No escribe; sale con código != 0 si algún archivo no está formateado.
    #[arg(long)]
    pub check: bool,

    /// Lee el contenido de stdin y escribe el resultado formateado en stdout.
    #[arg(long)]
    pub stdin: bool,

    /// Nombre del archivo para inferir el tipo al usar `--stdin`
    /// (`database.model` = globales; cualquier otro = entidad).
    #[arg(long, default_value = "entity.model")]
    pub stdin_name: String,
}

pub fn run(args: FmtArgs) -> Result<()> {
    if args.stdin {
        return run_stdin(&args.stdin_name);
    }

    let paths = if args.paths.is_empty() {
        vec![PathBuf::from("models")]
    } else {
        args.paths.clone()
    };

    let mut files = Vec::new();
    for p in &paths {
        collect_models(p, &mut files)
            .with_context(|| format!("recolectando .model en {}", p.display()))?;
    }
    if files.is_empty() {
        println!("No se encontraron archivos .model.");
        return Ok(());
    }

    let mut changed = 0usize;
    let mut errors = 0usize;
    for path in &files {
        match format_one(path, args.check) {
            Ok(true) => {
                changed += 1;
                if args.check {
                    println!("  ✗ sin formatear: {}", path.display());
                } else {
                    println!("  ✔ formateado: {}", path.display());
                }
            }
            Ok(false) => {}
            Err(e) => {
                errors += 1;
                eprintln!("  ⚠ {}: {e}", path.display());
            }
        }
    }

    if errors > 0 {
        anyhow::bail!("{errors} archivo(s) con errores de parseo");
    }
    if args.check {
        if changed > 0 {
            anyhow::bail!("{changed} archivo(s) sin formatear");
        }
        println!("Todos los archivos ya están formateados.");
    } else {
        println!(
            "Listo: {changed} archivo(s) reformateado(s) de {}.",
            files.len()
        );
    }
    Ok(())
}

/// Formatea un único archivo. Devuelve `true` si el contenido cambió (o cambiaría
/// con `--check`).
fn format_one(path: &Path, check: bool) -> Result<bool> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("entity.model");
    let src =
        std::fs::read_to_string(path).with_context(|| format!("leyendo {}", path.display()))?;
    let formatted = efrust_lang::format_model(name, &src).map_err(anyhow::Error::new)?;
    if formatted == src {
        return Ok(false);
    }
    if !check {
        std::fs::write(path, &formatted)
            .with_context(|| format!("escribiendo {}", path.display()))?;
    }
    Ok(true)
}

fn run_stdin(name: &str) -> Result<()> {
    let mut src = String::new();
    std::io::stdin()
        .read_to_string(&mut src)
        .context("leyendo stdin")?;
    let formatted = efrust_lang::format_model(name, &src).map_err(anyhow::Error::new)?;
    std::io::stdout()
        .write_all(formatted.as_bytes())
        .context("escribiendo stdout")?;
    Ok(())
}

/// Junta los `.model` de una ruta (archivo suelto o directorio, no recursivo).
fn collect_models(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if path.is_dir() {
        for entry in std::fs::read_dir(path)? {
            let p = entry?.path();
            if p.extension().and_then(|s| s.to_str()) == Some("model") {
                out.push(p);
            }
        }
    } else if path.extension().and_then(|s| s.to_str()) == Some("model") {
        out.push(path.to_path_buf());
    } else {
        anyhow::bail!(
            "no es un archivo .model ni un directorio: {}",
            path.display()
        );
    }
    Ok(())
}
