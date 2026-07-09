//! Updater glue: channel-aware endpoint selection for Tauri updater.

use crate::error::{ErrorCode, TypexError};
use crate::settings::schema::UpdateChannel;
use tauri::{AppHandle, Runtime, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

const STABLE_ENDPOINTS: &[&str] =
    &["https://github.com/typex-ink/Typex/releases/latest/download/latest.json"];

const NIGHTLY_ENDPOINTS: &[&str] =
    &["https://github.com/typex-ink/Typex/releases/download/nightly/latest.json"];

fn endpoint_urls(channel: UpdateChannel) -> Result<Vec<Url>, TypexError> {
    let endpoints = match channel {
        UpdateChannel::Stable => STABLE_ENDPOINTS,
        UpdateChannel::Nightly => NIGHTLY_ENDPOINTS,
    };
    endpoints
        .iter()
        .map(|endpoint| {
            endpoint.parse::<Url>().map_err(|e| {
                TypexError::new(
                    ErrorCode::NotConfigured,
                    format!("更新源地址无效 {endpoint}: {e}"),
                )
            })
        })
        .collect()
}

pub async fn check<R: Runtime>(
    app: &AppHandle<R>,
    channel: UpdateChannel,
) -> Result<Option<Update>, TypexError> {
    let endpoints = endpoint_urls(channel)?;
    let mut builder = app
        .updater_builder()
        .endpoints(endpoints)
        .map_err(|e| TypexError::new(ErrorCode::NotConfigured, format!("updater 未配置: {e}")))?;

    if channel == UpdateChannel::Nightly {
        builder = builder.version_comparator(|current, remote| {
            if remote.version > current {
                return true;
            }
            if remote.version < current {
                return false;
            }

            match (option_env!("GITHUB_SHA"), remote.notes.as_deref()) {
                (Some(current_commit), Some(notes)) if !current_commit.is_empty() => {
                    !notes.contains(current_commit)
                }
                _ => false,
            }
        });
    }

    let updater = builder
        .build()
        .map_err(|e| TypexError::new(ErrorCode::NotConfigured, format!("updater 未配置: {e}")))?;
    updater.check().await.map_err(|e| {
        TypexError::new(
            ErrorCode::NetworkError,
            format!("检查更新失败（{channel:?}）: {e}"),
        )
    })
}
