use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};
use rmcp::schemars;
use schemars::JsonSchema;
use serde::Deserialize;
use tracing::{info, instrument};

use super::DebugServer;
use crate::error::AppError;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateIssueParams {
    /// Issue title (max 256 characters).
    pub title: String,

    /// Issue body in Markdown. Include reproduction steps for bugs.
    pub body: String,

    /// Labels to apply (e.g. "bug", "enhancement"). Subject to allowed-labels config.
    #[serde(default)]
    pub labels: Vec<String>,
}

impl DebugServer {
    #[instrument(skip(self, params))]
    pub async fn handle_create_issue(
        &self,
        params: CreateIssueParams,
    ) -> Result<CallToolResult, McpError> {
        let config = &self.state.config;

        let repo = &config.github_repo;
        if repo.is_empty() {
            return Err(McpError::from(AppError::IssueCreationDisabled));
        }

        let title = params.title.trim().to_string();
        if title.is_empty() || title.len() > 256 {
            return Err(McpError::invalid_params(
                "title must be 1-256 characters",
                None,
            ));
        }

        let allowed = &config.github_allowed_labels;
        if !allowed.is_empty() {
            for label in &params.labels {
                if !allowed.contains(label) {
                    return Err(McpError::from(AppError::LabelNotAllowed {
                        label: label.clone(),
                        allowed: allowed.join(", "),
                    }));
                }
            }
        }

        let token = std::env::var("GITHUB_TOKEN")
            .map_err(|_| AppError::GitHubTokenMissing)?;

        let client = reqwest::Client::new();
        let url = format!("https://api.github.com/repos/{repo}/issues");

        let response = client
            .post(&url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .header("User-Agent", "mcp-dap")
            .bearer_auth(&token)
            .json(&serde_json::json!({
                "title": title,
                "body": params.body,
                "labels": params.labels,
            }))
            .send()
            .await
            .map_err(|e| AppError::GitHubApi {
                status: e.status().map_or(0, |s| s.as_u16()),
                message: e.to_string(),
            })
            .map_err(McpError::from)?;

        let status = response.status();
        if !status.is_success() {
            let body_text = response.text().await.unwrap_or_default();
            let message = serde_json::from_str::<serde_json::Value>(&body_text)
                .ok()
                .and_then(|v| v.get("message")?.as_str().map(String::from))
                .unwrap_or(body_text);
            return Err(McpError::from(AppError::GitHubApi {
                status: status.as_u16(),
                message,
            }));
        }

        let resp_body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| AppError::GitHubApi {
                status: 0,
                message: format!("failed to parse response: {e}"),
            })
            .map_err(McpError::from)?;

        let issue_url = resp_body
            .get("html_url")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)");
        let issue_number = resp_body
            .get("number")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);

        info!(issue_number, %issue_url, "GitHub issue created");

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Issue #{issue_number} created: {issue_url}",
        ))]))
    }
}
