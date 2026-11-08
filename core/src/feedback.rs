//! Feedback. The only request Recall itself sends anywhere, and only when the
//! person presses Send (the sign-in windows talk to the chat sites, as a
//! browser would). The body is the text they typed plus the app version so
//! a report can be matched to a build. No identifiers, no usage data.

use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Serialize)]
struct Body<'a> {
    text: &'a str,
    version: &'a str,
    platform: &'a str,
}

/// `endpoint` comes from the build (`RECALL_FEEDBACK_URL` at compile time); an
/// empty endpoint disables the feature and the UI hides the button.
pub fn send(endpoint: &str, text: &str, version: &str) -> Result<()> {
    anyhow::ensure!(!endpoint.is_empty(), "feedback endpoint not configured");
    anyhow::ensure!(endpoint.starts_with("https://"), "feedback endpoint must use https");
    let text = text.trim();
    anyhow::ensure!(!text.is_empty(), "nothing to send");
    anyhow::ensure!(text.len() <= 8000, "feedback is limited to 8000 characters");
    let body = Body { text, version, platform: std::env::consts::OS };
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(20))).build().into();
    agent
        .post(endpoint)
        .header("content-type", "application/json")
        .send(serde_json::to_string(&body)?.as_bytes())
        .context("send feedback")?;
    Ok(())
}
