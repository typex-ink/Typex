//! 文本注入：trait Injector + 后备链（07 §7.5）。
pub mod paste;

use crate::error::Result;

pub trait Injector: Send + Sync {
    /// 把文本注入当前焦点位置。
    fn inject(&self, text: &str) -> Result<()>;
    fn name(&self) -> &'static str;
}

/// 后备链：每个后端失败自动尝试下一个；全失败由 orchestrator 走「复制到剪贴板 + 提示」兜底。
pub struct InjectorChain {
    backends: Vec<Box<dyn Injector>>,
}

impl InjectorChain {
    pub fn new(backends: Vec<Box<dyn Injector>>) -> Self {
        Self { backends }
    }

    /// 平台默认链（07 §7.5）。
    pub fn platform_default(paste_delay_ms: u64) -> Self {
        Self::new(vec![Box::new(paste::PasteInjector::new(paste_delay_ms))])
    }

    pub fn inject(&self, text: &str) -> Result<()> {
        let mut last_err = None;
        for backend in &self.backends {
            match backend.inject(text) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!(
                        "注入后端 {} 失败: {}，尝试下一个",
                        backend.name(),
                        e.message
                    );
                    last_err = Some(e);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            crate::error::TypexError::new(crate::error::ErrorCode::Internal, "无可用注入后端")
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{ErrorCode, TypexError};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct MockInjector {
        fail: bool,
        calls: Arc<AtomicU32>,
    }

    impl Injector for MockInjector {
        fn inject(&self, _text: &str) -> Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(TypexError::new(ErrorCode::Internal, "mock fail"))
            } else {
                Ok(())
            }
        }
        fn name(&self) -> &'static str {
            "mock"
        }
    }

    #[test]
    fn chain_stops_at_first_success() {
        let c1 = Arc::new(AtomicU32::new(0));
        let c2 = Arc::new(AtomicU32::new(0));
        let chain = InjectorChain::new(vec![
            Box::new(MockInjector {
                fail: false,
                calls: c1.clone(),
            }),
            Box::new(MockInjector {
                fail: false,
                calls: c2.clone(),
            }),
        ]);
        chain.inject("hi").unwrap();
        assert_eq!(c1.load(Ordering::SeqCst), 1);
        assert_eq!(c2.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn chain_falls_through_on_failure() {
        let c1 = Arc::new(AtomicU32::new(0));
        let c2 = Arc::new(AtomicU32::new(0));
        let chain = InjectorChain::new(vec![
            Box::new(MockInjector {
                fail: true,
                calls: c1.clone(),
            }),
            Box::new(MockInjector {
                fail: false,
                calls: c2.clone(),
            }),
        ]);
        chain.inject("hi").unwrap();
        assert_eq!(c1.load(Ordering::SeqCst), 1);
        assert_eq!(c2.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn all_failed_returns_error() {
        let c = Arc::new(AtomicU32::new(0));
        let chain = InjectorChain::new(vec![Box::new(MockInjector {
            fail: true,
            calls: c.clone(),
        })]);
        assert!(chain.inject("hi").is_err());
    }
}
