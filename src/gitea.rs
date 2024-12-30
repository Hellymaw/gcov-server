use lazy_static::lazy_static;
use serde::Serialize;
use std::path::Path;

lazy_static! {
    static ref GITEA_API_KEY: String = std::env::var("GITEA_API_KEY").unwrap();
}

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

pub mod repository {
    use crate::gitea::GITEA_API_KEY;
    use base64::{prelude::BASE64_STANDARD, Engine as _};
    use serde::{de, Deserialize};

    #[derive(Debug, Deserialize)]
    #[serde(rename_all = "lowercase", tag = "type", content = "name")]
    pub enum DirectoryEntries {
        #[serde(rename = "dir")]
        Directory(String),
        File(String),
        Symlink(String),
        Submodule(String),
    }

    impl DirectoryEntries {
        pub fn name(&self) -> &str {
            match self {
                DirectoryEntries::Directory(name)
                | DirectoryEntries::File(name)
                | DirectoryEntries::Symlink(name)
                | DirectoryEntries::Submodule(name) => name,
            }
        }
    }

    #[derive(Debug)]
    pub enum Entry {
        File { content: String },
        Directory(Vec<DirectoryEntries>),
        Symlink { target: String },
        Submodule { url: String },
    }

    impl<'de> Deserialize<'de> for Entry {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            #[derive(Debug, Deserialize)]
            struct ContentsResponse {
                r#type: String,
                content: Option<String>,
                _encoding: Option<String>,
                submodule_git_url: Option<String>,
                target: Option<String>,
            }

            let json = serde_json::Value::deserialize(deserializer)?;

            match json {
                serde_json::Value::Array(_) => {
                    let entries: Vec<DirectoryEntries> =
                        serde_json::from_value(json).map_err(de::Error::custom)?;

                    Ok(Self::Directory(entries))
                }
                serde_json::Value::Object(_) => {
                    tracing::info!("json: {json:?}");

                    let mut resp: ContentsResponse =
                        serde_json::from_value(json).map_err(de::Error::custom)?;

                    tracing::info!("Resp: {resp:?}");

                    match resp.r#type.as_str() {
                        "file" => {
                            if let Some(content) = resp.content.take() {
                                match BASE64_STANDARD.decode(&content) {
                                    Ok(decoded) => {
                                        if let Ok(content) = String::from_utf8(decoded) {
                                            Ok(Self::File { content })
                                        } else {
                                            Err(de::Error::invalid_value(
                                                de::Unexpected::Str(&content),
                                                &"a utf-8 encoded value",
                                            ))
                                        }
                                    }
                                    Err(x) => Err(de::Error::invalid_value(
                                        de::Unexpected::Str(&content),
                                        &format!("a base64 encoded value but had: {x}").as_str(),
                                    )),
                                }
                            } else {
                                Err(de::Error::missing_field("content"))
                            }
                        }
                        "symlink" => {
                            if let Some(target) = resp.target.take() {
                                Ok(Self::Symlink { target })
                            } else {
                                Err(de::Error::missing_field("target"))
                            }
                        }
                        "submodule" => {
                            if let Some(url) = resp.submodule_git_url.take() {
                                Ok(Self::Submodule { url })
                            } else {
                                Err(de::Error::missing_field("submodule_git_url"))
                            }
                        }
                        r#type => Err(de::Error::unknown_variant(
                            r#type,
                            &["file", "symlink", "submodule"],
                        )),
                    }
                }
                _ => Err(de::Error::custom("a JSON object or array")), // TODO: Something a bit better than this
            }
        }
    }

    pub async fn get_repository_entries(
        owner: &str,
        repository: &str,
        reference: Option<&str>,
        filepath: &str,
    ) -> Result<Entry, reqwest::Error> {
        // TODO: validate filepath

        // The API scheme requires the root dir to fetch without a trailing '/'. So remove it if given
        let filepath = if filepath == "/" { "" } else { filepath };
        let mut url =
            format!("http://localhost:3000/api/v1/repos/{owner}/{repository}/contents{filepath}");
        if let Some(reference) = reference {
            url.push_str("?ref=");
            url.push_str(reference);
        }

        tracing::info!("Requesting \'{filepath}\' from {owner}/{repository}");

        reqwest::Client::new()
            .get(url)
            .header("Authorization", GITEA_API_KEY.as_str())
            .send()
            .await?
            .json::<Entry>()
            .await
    }
}
