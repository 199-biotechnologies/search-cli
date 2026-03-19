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
