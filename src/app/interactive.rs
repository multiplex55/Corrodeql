use std::path::PathBuf;

use anyhow::Result;
use inquire::{Confirm, Select, Text};

use super::cli::{ConvertArgs, TableNameMode};

const DEFAULT_NULL_TOKEN: &str = r"\N";

/// Fully resolved options for a conversion run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConvertOptions {
    pub schema: PathBuf,
    pub data_dir: PathBuf,
    pub out: PathBuf,
    pub overwrite: bool,
    pub null_token: String,
    pub table_name_mode: TableNameMode,
    pub emit_ddl: Option<PathBuf>,
    pub report_dir: Option<PathBuf>,
    pub strict: bool,
    pub allow_missing_csv: bool,
    pub allow_extra_csv_columns: bool,
    pub skip_foreign_key_check: bool,
    pub dry_run: bool,
}

/// Convert setup prompts that may be required before a complete conversion can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConvertPrompt {
    SchemaPath,
    DataDirectory,
    OutputPath,
}

/// Answers collected from the interactive convert setup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConvertPromptAnswers {
    pub schema: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub out: Option<PathBuf>,
    pub overwrite: Option<bool>,
    pub null_token: Option<String>,
    pub table_name_mode: Option<TableNameMode>,
    pub emit_ddl: Option<Option<PathBuf>>,
    pub report_dir: Option<Option<PathBuf>>,
    pub strict: Option<bool>,
}

/// Returns the required conversion prompts missing from the CLI arguments.
pub fn missing_convert_prompts(args: &ConvertArgs) -> Vec<ConvertPrompt> {
    let mut prompts = Vec::new();

    if args.schema.is_none() {
        prompts.push(ConvertPrompt::SchemaPath);
    }
    if args.data_dir.is_none() {
        prompts.push(ConvertPrompt::DataDirectory);
    }
    if args.out.is_none() {
        prompts.push(ConvertPrompt::OutputPath);
    }

    prompts
}

/// Merges CLI arguments with prompt answers into complete conversion options.
pub fn merge_convert_options(
    args: ConvertArgs,
    answers: ConvertPromptAnswers,
) -> Result<ConvertOptions> {
    let schema = args
        .schema
        .or(answers.schema)
        .ok_or_else(|| anyhow::anyhow!("schema file path is required"))?;
    let data_dir = args
        .data_dir
        .or(answers.data_dir)
        .ok_or_else(|| anyhow::anyhow!("CSV data directory is required"))?;
    let out = args
        .out
        .or(answers.out)
        .ok_or_else(|| anyhow::anyhow!("output SQLite path is required"))?;

    Ok(ConvertOptions {
        schema,
        data_dir,
        out,
        overwrite: args.overwrite || answers.overwrite.unwrap_or(false),
        null_token: args
            .null_token
            .or(answers.null_token)
            .unwrap_or_else(|| DEFAULT_NULL_TOKEN.to_string()),
        table_name_mode: args
            .table_name_mode
            .or(answers.table_name_mode)
            .unwrap_or(TableNameMode::SchemaPrefix),
        emit_ddl: args.emit_ddl.or_else(|| answers.emit_ddl.flatten()),
        report_dir: args.report_dir.or_else(|| answers.report_dir.flatten()),
        strict: args.strict || answers.strict.unwrap_or(false),
        allow_missing_csv: args.allow_missing_csv,
        allow_extra_csv_columns: args.allow_extra_csv_columns,
        skip_foreign_key_check: args.skip_foreign_key_check,
        dry_run: args.dry_run,
    })
}

/// Completes missing conversion options using interactive prompts when required.
pub fn complete_convert_options(args: ConvertArgs) -> Result<ConvertOptions> {
    if missing_convert_prompts(&args).is_empty() {
        return merge_convert_options(args, ConvertPromptAnswers::default());
    }

    let answers = prompt_for_convert_options(&args)?;
    merge_convert_options(args, answers)
}

fn prompt_for_convert_options(args: &ConvertArgs) -> Result<ConvertPromptAnswers> {
    let mut answers = ConvertPromptAnswers::default();

    if args.schema.is_none() {
        answers.schema = Some(PathBuf::from(Text::new("Schema file path:").prompt()?));
    }
    if args.data_dir.is_none() {
        answers.data_dir = Some(PathBuf::from(Text::new("CSV data directory:").prompt()?));
    }
    if args.out.is_none() {
        answers.out = Some(PathBuf::from(Text::new("Output SQLite path:").prompt()?));
    }

    if !args.overwrite {
        answers.overwrite = Some(
            Confirm::new("Overwrite output if it exists?")
                .with_default(false)
                .prompt()?,
        );
    }
    if args.null_token.is_none() {
        answers.null_token = Some(
            Text::new("Null token:")
                .with_default(DEFAULT_NULL_TOKEN)
                .prompt()?,
        );
    }
    if args.table_name_mode.is_none() {
        answers.table_name_mode = Some(prompt_table_name_mode()?);
    }
    if args.emit_ddl.is_none() {
        let emit = Confirm::new("Emit converted SQLite DDL?")
            .with_default(false)
            .prompt()?;
        answers.emit_ddl = Some(if emit {
            Some(PathBuf::from(
                Text::new("SQLite DDL output path:").prompt()?,
            ))
        } else {
            None
        });
    }
    if args.report_dir.is_none() {
        let reports = Confirm::new("Produce reports?")
            .with_default(false)
            .prompt()?;
        answers.report_dir = Some(if reports {
            Some(PathBuf::from(
                Text::new("Report output directory:").prompt()?,
            ))
        } else {
            None
        });
    }
    if !args.strict {
        answers.strict = Some(
            Confirm::new("Use strict mode?")
                .with_default(false)
                .prompt()?,
        );
    }

    Ok(answers)
}

fn prompt_table_name_mode() -> Result<TableNameMode> {
    let selected = Select::new(
        "Table naming mode:",
        vec!["schema-prefix", "drop-dbo", "table-only"],
    )
    .prompt()?;

    Ok(match selected {
        "schema-prefix" => TableNameMode::SchemaPrefix,
        "drop-dbo" => TableNameMode::DropDbo,
        "table-only" => TableNameMode::TableOnly,
        _ => unreachable!("selection must come from fixed choices"),
    })
}

/// Starts an interactive convert setup with default CLI arguments.
pub fn start() -> Result<ConvertOptions> {
    complete_convert_options(ConvertArgs::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_args() -> ConvertArgs {
        ConvertArgs {
            schema: Some(PathBuf::from("schema.sql")),
            data_dir: Some(PathBuf::from("data")),
            out: Some(PathBuf::from("output.sqlite")),
            overwrite: true,
            null_token: Some("NULL".to_string()),
            table_name_mode: Some(TableNameMode::DropDbo),
            emit_ddl: Some(PathBuf::from("ddl.sql")),
            report_dir: Some(PathBuf::from("reports")),
            strict: true,
            allow_missing_csv: true,
            allow_extra_csv_columns: true,
            skip_foreign_key_check: true,
            dry_run: true,
        }
    }

    #[test]
    fn full_cli_arguments_produce_no_missing_prompts() {
        assert!(missing_convert_prompts(&full_args()).is_empty());
    }

    #[test]
    fn missing_schema_path_requests_schema_prompt() {
        let mut args = full_args();
        args.schema = None;
        assert_eq!(
            missing_convert_prompts(&args),
            vec![ConvertPrompt::SchemaPath]
        );
    }

    #[test]
    fn missing_data_directory_requests_data_directory_prompt() {
        let mut args = full_args();
        args.data_dir = None;
        assert_eq!(
            missing_convert_prompts(&args),
            vec![ConvertPrompt::DataDirectory]
        );
    }

    #[test]
    fn missing_output_path_requests_output_prompt() {
        let mut args = full_args();
        args.out = None;
        assert_eq!(
            missing_convert_prompts(&args),
            vec![ConvertPrompt::OutputPath]
        );
    }

    #[test]
    fn mixed_mode_preserves_provided_values_and_prompts_only_for_missing_values() {
        let args = ConvertArgs {
            schema: Some(PathBuf::from("schema.sql")),
            data_dir: None,
            out: Some(PathBuf::from("output.sqlite")),
            ..ConvertArgs::default()
        };

        assert_eq!(
            missing_convert_prompts(&args),
            vec![ConvertPrompt::DataDirectory]
        );

        let options = merge_convert_options(
            args,
            ConvertPromptAnswers {
                data_dir: Some(PathBuf::from("prompted-data")),
                overwrite: Some(true),
                table_name_mode: Some(TableNameMode::TableOnly),
                ..ConvertPromptAnswers::default()
            },
        )
        .expect("merged options should be complete");

        assert_eq!(options.schema, PathBuf::from("schema.sql"));
        assert_eq!(options.data_dir, PathBuf::from("prompted-data"));
        assert_eq!(options.out, PathBuf::from("output.sqlite"));
        assert!(options.overwrite);
        assert_eq!(options.null_token, DEFAULT_NULL_TOKEN);
        assert_eq!(options.table_name_mode, TableNameMode::TableOnly);
    }
}
