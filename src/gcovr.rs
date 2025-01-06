use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct FileEntry {
    pub filename: String,
    pub lines: Vec<LineEntry>,
    // functions: Vec<FunctionEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LineEntry {
    pub line_number: usize,
    // function_name: String,
    pub count: usize,
    pub branches: Vec<BranchEntry>,
    // conditions: Vec<ConditionEntry>,
    // block_ids: Vec<i64>,
    // #[serde(rename = "gcovr/md5")]
    // md5: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BranchEntry {
    pub count: usize,
    fallthrough: bool,
    throw: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct ConditionEntry {
    count: usize,
    covered: usize,
    not_covered_false: Vec<usize>,
    not_covered_true: Vec<usize>,
}

#[derive(Debug, Deserialize, Serialize)]
struct FunctionEntry {
    name: String,
    lineno: usize,
    execution_count: usize,
    branch_percent: f64,
}
