pub mod json;
pub mod table;

use std::io::IsTerminal;

pub enum OutputFormat {
    Json,
    Table,
}

impl OutputFormat {
    pub fn detect(json_flag: bool) -> Self {
        if json_flag || !std::io::stdout().is_terminal() {
            OutputFormat::Json
        } else {
            OutputFormat::Table
        }
    }
}

/// Output context: bundles format + quiet so dispatch sites take one value.
pub struct Ctx {
    pub format: OutputFormat,
    pub quiet: bool,
}

impl Ctx {
    pub fn new(json_flag: bool, quiet: bool) -> Self {
        Self {
            format: OutputFormat::detect(json_flag),
            quiet,
        }
    }

    pub fn is_json(&self) -> bool {
        matches!(self.format, OutputFormat::Json)
    }

    /// True when human output should be suppressed (quiet + table mode).
    pub fn suppress_human(&self) -> bool {
        self.quiet && !self.is_json()
    }
}
