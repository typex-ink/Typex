//! 读取选中文本：trait SelectionReader + 平台降级链（07 §7.6）。CP-3.1 实现。

use crate::error::Result;

pub trait SelectionReader: Send + Sync {
    /// 读取当前选中文本；None = 无选区。
    fn read(&self) -> Result<Option<String>>;
}
