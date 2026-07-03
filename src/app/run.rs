use std::path::Path;
use std::process::ExitCode;

use anyhow::{bail, Result};
use clap::Parser;

use super::cli::{Cli, Command, EmitDdlArgs, InitExampleArgs, InspectSchemaArgs, ValidateArgs};
use super::interactive::{complete_convert_options, ConvertOptions};

/// Runs the CorrodeQL command-line application.
pub fn run() -> ExitCode {
    match run_with_args(std::env::args_os()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

/// Parses command-line arguments and routes the selected command.
pub fn run_with_args<I, T>(args: I) -> Result<()>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let cli = Cli::parse_from(args);

    match cli.command {
        Some(Command::Convert(args)) => run_convert(args),
        Some(Command::InspectSchema(args)) => run_inspect_schema(args),
        Some(Command::EmitDdl(args)) => run_emit_ddl(args),
        Some(Command::Validate(args)) => run_validate(args),
        Some(Command::InitExample(args)) => run_init_example(args),
        None => run_convert(Default::default()),
    }
}

/// Placeholder implementation for `corrodeql convert`.
pub fn run_convert(args: super::cli::ConvertArgs) -> Result<()> {
    let options = complete_convert_options(args)?;
    run_convert_with_options(options)
}

fn run_convert_with_options(options: ConvertOptions) -> Result<()> {
    validate_schema_path(Some(options.schema.as_path()))?;
    validate_data_dir(Some(options.data_dir.as_path()))?;
    validate_output_parent(Some(options.out.as_path()))?;
    validate_output_parent(options.emit_ddl.as_deref())?;

    if options.dry_run {
        println!("convert dry run: arguments parsed and obvious paths validated");
    } else {
        println!("convert is not yet implemented; no conversion was attempted");
    }

    Ok(())
}

/// Placeholder implementation for `corrodeql inspect-schema`.
pub fn run_inspect_schema(args: InspectSchemaArgs) -> Result<()> {
    validate_schema_path(args.schema.as_deref())?;
    println!("inspect-schema is not yet implemented; no schema inspection was attempted");
    Ok(())
}

/// Placeholder implementation for `corrodeql emit-ddl`.
pub fn run_emit_ddl(args: EmitDdlArgs) -> Result<()> {
    validate_schema_path(args.schema.as_deref())?;
    validate_output_parent(args.out.as_deref())?;
    println!("emit-ddl is not yet implemented; no DDL was emitted");
    Ok(())
}

/// Placeholder implementation for `corrodeql validate`.
pub fn run_validate(args: ValidateArgs) -> Result<()> {
    validate_schema_path(args.schema.as_deref())?;
    validate_data_dir(args.data_dir.as_deref())?;
    println!("validate is not yet implemented; no validation was attempted");
    Ok(())
}

/// Placeholder implementation for `corrodeql init-example`.
pub fn run_init_example(_args: InitExampleArgs) -> Result<()> {
    println!("init-example is not yet implemented; no files were written");
    Ok(())
}

fn validate_schema_path(path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        if !path.exists() {
            bail!("schema path does not exist: {}", path.display());
        }
        if !path.is_file() {
            bail!("schema path must be a file: {}", path.display());
        }
    }

    Ok(())
}

fn validate_data_dir(path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        if !path.exists() {
            bail!("data directory does not exist: {}", path.display());
        }
        if !path.is_dir() {
            bail!("data directory must be a directory: {}", path.display());
        }
    }

    Ok(())
}

fn validate_output_parent(path: Option<&Path>) -> Result<()> {
    if let Some(path) = path {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            if !parent.exists() {
                bail!(
                    "output parent directory does not exist: {}",
                    parent.display()
                );
            }
            if !parent.is_dir() {
                bail!(
                    "output parent path must be a directory: {}",
                    parent.display()
                );
            }
        }
    }

    Ok(())
}
