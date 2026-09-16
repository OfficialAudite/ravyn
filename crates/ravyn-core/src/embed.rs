use serde::{Deserialize, Serialize};

/// Per-user Discord/Slack/Twitter embed settings for the `/v/{id}` view
/// page. Modeled after Zipline's `view.embed*` fields.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EmbedSettings {
    pub enabled: bool,
    pub title: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub site_name: Option<String>,
}
