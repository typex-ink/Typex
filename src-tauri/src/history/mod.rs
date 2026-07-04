//! 历史记录（F-7）：rusqlite WAL；建表/迁移/CRUD/保留期清理/主页统计聚合。
//! 仅存本机、不含音频（ADR-8）。

use crate::error::{ErrorCode, Result, TypexError};
use crate::types::SessionMode;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct HistoryItem {
    pub id: i64,
    /// Unix 毫秒
    pub created_at: i64,
    pub mode: SessionMode,
    /// 原始转写
    pub transcript: String,
    /// 整理后/译文/回答（原样模式下与 transcript 相同）
    pub result: String,
    /// 目标应用名（可空）
    pub app_name: String,
    /// 录音时长 ms（统计口径，ADR-19）
    pub duration_ms: u32,
    /// 上屏字数
    pub char_count: u32,
}

/// 主页统计（05 §8：本地聚合，零上报）。
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct HistoryStats {
    pub total_duration_ms: f64,
    pub total_chars: f64,
    pub session_count: f64,
}

fn mode_str(m: SessionMode) -> &'static str {
    match m {
        SessionMode::Dictation => "dictation",
        SessionMode::Translation => "translation",
        SessionMode::Assistant => "assistant",
    }
}

fn mode_of(s: &str) -> SessionMode {
    match s {
        "translation" => SessionMode::Translation,
        "assistant" => SessionMode::Assistant,
        _ => SessionMode::Dictation,
    }
}

pub struct HistoryService {
    conn: Mutex<Connection>,
}

impl HistoryService {
    pub fn open(db_path: &Path) -> Result<Self> {
        if let Some(dir) = db_path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                TypexError::new(ErrorCode::Internal, format!("创建数据目录失败: {e}"))
            })?;
        }
        let conn = Connection::open(db_path)
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("打开历史库失败: {e}")))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("WAL 失败: {e}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at INTEGER NOT NULL,
                mode TEXT NOT NULL,
                transcript TEXT NOT NULL,
                result TEXT NOT NULL,
                app_name TEXT NOT NULL DEFAULT '',
                duration_ms INTEGER NOT NULL DEFAULT 0,
                char_count INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_history_created ON history(created_at DESC);",
        )
        .map_err(|e| TypexError::new(ErrorCode::Internal, format!("建表失败: {e}")))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .map_err(|e| TypexError::new(ErrorCode::Internal, e.to_string()))?;
        conn.execute_batch(
            "CREATE TABLE history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at INTEGER NOT NULL,
                mode TEXT NOT NULL,
                transcript TEXT NOT NULL,
                result TEXT NOT NULL,
                app_name TEXT NOT NULL DEFAULT '',
                duration_ms INTEGER NOT NULL DEFAULT 0,
                char_count INTEGER NOT NULL DEFAULT 0
            );",
        )
        .map_err(|e| TypexError::new(ErrorCode::Internal, e.to_string()))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(
        &self,
        created_at: i64,
        mode: SessionMode,
        transcript: &str,
        result: &str,
        app_name: &str,
        duration_ms: u32,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO history (created_at, mode, transcript, result, app_name, duration_ms, char_count)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                created_at,
                mode_str(mode),
                transcript,
                result,
                app_name,
                duration_ms,
                result.chars().count() as u32,
            ],
        )
        .map_err(|e| TypexError::new(ErrorCode::Internal, format!("写历史失败: {e}")))?;
        Ok(())
    }

    /// 搜索 + 分页（时间倒序）。
    pub fn query(&self, search: &str, offset: u32, limit: u32) -> Result<Vec<HistoryItem>> {
        let conn = self.conn.lock().unwrap();
        let like = format!("%{search}%");
        let mut stmt = conn
            .prepare(
                "SELECT id, created_at, mode, transcript, result, app_name, duration_ms, char_count
                 FROM history
                 WHERE transcript LIKE ?1 OR result LIKE ?1
                 ORDER BY created_at DESC LIMIT ?2 OFFSET ?3",
            )
            .map_err(|e| TypexError::new(ErrorCode::Internal, e.to_string()))?;
        let rows = stmt
            .query_map(rusqlite::params![like, limit, offset], |row| {
                Ok(HistoryItem {
                    id: row.get(0)?,
                    created_at: row.get(1)?,
                    mode: mode_of(&row.get::<_, String>(2)?),
                    transcript: row.get(3)?,
                    result: row.get(4)?,
                    app_name: row.get(5)?,
                    duration_ms: row.get(6)?,
                    char_count: row.get(7)?,
                })
            })
            .map_err(|e| TypexError::new(ErrorCode::Internal, e.to_string()))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(rows)
    }

    pub fn stats(&self) -> Result<HistoryStats> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(SUM(duration_ms),0), COALESCE(SUM(char_count),0), COUNT(*) FROM history",
            [],
            |row| {
                Ok(HistoryStats {
                    total_duration_ms: row.get::<_, i64>(0)? as f64,
                    total_chars: row.get::<_, i64>(1)? as f64,
                    session_count: row.get::<_, i64>(2)? as f64,
                })
            },
        )
        .map_err(|e| TypexError::new(ErrorCode::Internal, e.to_string()))
    }

    pub fn delete(&self, id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM history WHERE id = ?1", [id])
            .map_err(|e| TypexError::new(ErrorCode::Internal, e.to_string()))?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM history", [])
            .map_err(|e| TypexError::new(ErrorCode::Internal, e.to_string()))?;
        Ok(())
    }

    /// 保留期清理（启动时跑）。retention_days = 0 表示永久。now_ms 注入便于测试。
    pub fn cleanup(&self, retention_days: u32, now_ms: i64) -> Result<u32> {
        if retention_days == 0 {
            return Ok(0);
        }
        let cutoff = now_ms - (retention_days as i64) * 86_400_000;
        let conn = self.conn.lock().unwrap();
        let n = conn
            .execute("DELETE FROM history WHERE created_at < ?1", [cutoff])
            .map_err(|e| TypexError::new(ErrorCode::Internal, e.to_string()))?;
        Ok(n as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc() -> HistoryService {
        HistoryService::open_in_memory().unwrap()
    }

    #[test]
    fn crud_roundtrip() {
        let h = svc();
        h.insert(
            1000,
            SessionMode::Dictation,
            "嗯原文",
            "干净结果",
            "微信",
            5000,
        )
        .unwrap();
        h.insert(
            2000,
            SessionMode::Translation,
            "中文",
            "English",
            "Slack",
            3000,
        )
        .unwrap();

        let all = h.query("", 0, 10).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].result, "English"); // 时间倒序
        assert_eq!(all[0].char_count, 7);

        let hits = h.query("干净", 0, 10).unwrap();
        assert_eq!(hits.len(), 1);

        h.delete(all[1].id).unwrap();
        assert_eq!(h.query("", 0, 10).unwrap().len(), 1);
        h.clear().unwrap();
        assert_eq!(h.query("", 0, 10).unwrap().len(), 0);
    }

    #[test]
    fn stats_aggregate() {
        let h = svc();
        h.insert(
            1,
            SessionMode::Dictation,
            "a",
            "十个字十个字十个字十",
            "x",
            60_000,
        )
        .unwrap();
        h.insert(2, SessionMode::Dictation, "b", "五字五字五", "x", 30_000)
            .unwrap();
        let s = h.stats().unwrap();
        assert_eq!(s.total_duration_ms, 90_000.0);
        assert_eq!(s.total_chars, 15.0);
        assert_eq!(s.session_count, 2.0);
    }

    #[test]
    fn retention_cleanup_with_injected_clock() {
        let h = svc();
        let now: i64 = 100 * 86_400_000; // 第 100 天
        h.insert(
            now - 91 * 86_400_000,
            SessionMode::Dictation,
            "旧",
            "旧",
            "",
            0,
        )
        .unwrap();
        h.insert(
            now - 5 * 86_400_000,
            SessionMode::Dictation,
            "新",
            "新",
            "",
            0,
        )
        .unwrap();
        let deleted = h.cleanup(90, now).unwrap();
        assert_eq!(deleted, 1);
        let rest = h.query("", 0, 10).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].result, "新");
    }

    #[test]
    fn retention_zero_means_forever() {
        let h = svc();
        h.insert(1, SessionMode::Dictation, "远古", "远古", "", 0)
            .unwrap();
        assert_eq!(h.cleanup(0, i64::MAX / 2).unwrap(), 0);
        assert_eq!(h.query("", 0, 10).unwrap().len(), 1);
    }
}
