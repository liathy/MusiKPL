use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct YtEntry {
    pub(crate) id: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) duration: Option<f64>,
    pub(crate) channel: Option<String>,
    pub(crate) uploader: Option<String>,
    pub(crate) webpage_url: Option<String>,
}
