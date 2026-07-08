//! 顶层错误与用户可见错误码（06 §5.4）。
//!
//! `ErrorCode` 是 IPC 契约的一部分：Rust 只发 code + 参数，
//! 文案在前端 i18n（05 §9 错误文案表以此为键）。

use serde::{Deserialize, Serialize};

/// 用户可见错误分类。命名与前端 i18n key 对齐（`error.<code>`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// 鉴权/访问控制失败（401/403）——不重试，引导检查密钥、权限、客户端分组
    AuthError,
    /// 网络不可达 / DNS / 连接被拒
    NetworkError,
    /// 服务响应超时
    Timeout,
    /// 429 —— 自动退避重试
    RateLimited,
    /// 5xx
    ServerError,
    /// 请求本身非法（参数/格式）
    InvalidRequest,
    /// 转写结果为空（纯静音）
    NoSpeech,
    /// 当前无输入焦点，结果已转剪贴板
    NoFocus,
    /// 缺少系统权限（辅助功能/麦克风/输入监听）
    PermissionMissing,
    /// 录音设备错误
    AudioDevice,
    /// 配置缺失（槽位未配置）
    NotConfigured,
    /// 其他内部错误
    Internal,
}

/// 顶层错误：所有 service 的局部错误最终收敛到这里。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize, specta::Type)]
#[error("{code:?}: {message}")]
pub struct TypexError {
    pub code: ErrorCode,
    /// 开发者可读信息（进日志；前端展示走 i18n，不直接显示此字段）
    pub message: String,
}

impl TypexError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

pub type Result<T> = std::result::Result<T, TypexError>;
