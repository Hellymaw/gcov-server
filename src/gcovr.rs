use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Deserialize, Serialize)]
pub struct FileEntry {
    pub filename: PathBuf,
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

pub fn fake_file_entry() -> FileEntry {
    let mut lines: Vec<LineEntry> = Vec::new();
    for i in 0..100 {
        let mut branches: Vec<BranchEntry> = Vec::new();
        for _ in 0..2 {
            branches.push(BranchEntry {
                count: 2,
                fallthrough: false,
                throw: false,
            });
        }

        lines.push(LineEntry {
            line_number: i,
            count: 2,
            branches,
        });
    }

    FileEntry {
        filename: "some_file".into(),
        lines,
    }
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
