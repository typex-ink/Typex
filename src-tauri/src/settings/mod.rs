//! SettingsService：读写、校验、变更广播（07 §5.1 / §9）。
pub mod schema;
pub mod secrets;

use crate::error::{ErrorCode, Result, TypexError};
use schema::Settings;
use std::path::PathBuf;
use tokio::sync::watch;

/// 设置服务：JSON 落盘（无密钥明文）+ watch 广播。
pub struct SettingsService {
    path: PathBuf,
    tx: watch::Sender<Settings>,
}

impl SettingsService {
    /// 从 `config_dir/settings.json` 加载；不存在或损坏时回退默认（损坏原文件保留 .bak）。
    pub fn load(config_dir: PathBuf) -> Self {
        let path = config_dir.join("settings.json");
        let settings = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("settings.json 解析失败，回退默认: {e}");
                    let _ = std::fs::rename(&path, path.with_extension("json.bak"));
                    Settings::default()
                }
            },
            Err(_) => Settings::default(),
        };
        let (tx, _) = watch::channel(settings);
        Self { path, tx }
    }

    pub fn get(&self) -> Settings {
        self.tx.borrow().clone()
    }

    pub fn subscribe(&self) -> watch::Receiver<Settings> {
        self.tx.subscribe()
    }

    /// 全量替换并落盘 + 广播。
    pub fn update(&self, new: Settings) -> Result<Settings> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| TypexError::new(ErrorCode::Internal, format!("创建配置目录失败: {e}")))?;
        }
        let json = serde_json::to_string_pretty(&new)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("序列化设置失败: {e}")))?;
        std::fs::write(&self.path, json)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("写入设置失败: {e}")))?;
        self.tx.send_replace(new.clone());
        Ok(new)
    }

    /// 就地修改（读-改-写）。
    pub fn mutate(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings> {
        let mut s = self.get();
        f(&mut s);
        self.update(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_returns_default_and_update_persists() {
        let dir = std::env::temp_dir().join(format!("typex-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let svc = SettingsService::load(dir.clone());
        assert_eq!(svc.get().schema_version, 1);

        svc.mutate(|s| s.general.autostart = false).unwrap();
        let svc2 = SettingsService::load(dir.clone());
        assert!(!svc2.get().general.autostart);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_falls_back_to_default_with_bak() {
        let dir = std::env::temp_dir().join(format!("typex-test-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), "{ not json").unwrap();
        let svc = SettingsService::load(dir.clone());
        assert_eq!(svc.get().schema_version, 1);
        assert!(dir.join("settings.json.bak").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn watch_broadcasts_change() {
        let dir = std::env::temp_dir().join(format!("typex-test-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let svc = SettingsService::load(dir.clone());
        let mut rx = svc.subscribe();
        svc.mutate(|s| s.dictation.polish_enabled = false).unwrap();
        assert!(rx.has_changed().unwrap());
        assert!(!rx.borrow_and_update().dictation.polish_enabled);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
