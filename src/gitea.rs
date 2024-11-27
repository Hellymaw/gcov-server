use std::path::Path;

pub async fn get_file(path: &Path) -> String {
    let file_data = "some\nfile\nwith\ndata";

    file_data.to_string()
}
