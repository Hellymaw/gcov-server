use serde::Serialize;
use std::path::Path;

pub async fn get_file(path: &Path) -> String {
    let file_data = "some\nfile\nwith\ndata";

    file_data.to_string()
}

#[derive(Debug, Serialize)]
pub struct FileType {
    t: String,
    name: String,
}

pub async fn get_repo_contents(org: &str, repo: &str, commit: &str) -> Vec<FileType> {
    vec![
        FileType {
            t: "file".to_string(),
            name: "test".to_string(),
        },
        FileType {
            t: "dir".to_string(),
            name: "dir".to_string(),
        },
    ]
}
