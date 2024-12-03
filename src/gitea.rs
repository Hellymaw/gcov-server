use serde::Serialize;
use std::path::Path;

pub async fn get_file(_path: &Path) -> String {
    let file_data = "some\nfile\nwith\ndata";

    file_data.to_string()
}

#[derive(Debug, Serialize)]
pub struct Summary(f64, f64, f64);

#[derive(Debug, Serialize)]
pub struct FileType {
    pub r#type: String,
    pub name: String,
    pub path: String,
    pub summary: Summary,
    pub is_dir: bool,
}

pub async fn get_repo_contents(_org: &str, _repo: &str, _commit: &str) -> Vec<FileType> {
    vec![
        FileType {
            r#type: "file".to_string(),
            name: "test".to_string(),
            path: "/test".to_string(),
            summary: Summary(30.2, 30.2, 30.2),
            is_dir: false,
        },
        FileType {
            r#type: "dir".to_string(),
            name: "dir".to_string(),
            path: "/dir".to_string(),
            summary: Summary(30.2, 30.2, 30.2),
            is_dir: true,
        },
    ]
}
