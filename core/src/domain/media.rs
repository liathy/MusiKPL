use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct YtEntry {
    pub id: Option<String>,
    pub title: Option<String>,
    pub duration: Option<f64>,
    pub channel: Option<String>,
    pub uploader: Option<String>,
    pub webpage_url: Option<String>,
}
