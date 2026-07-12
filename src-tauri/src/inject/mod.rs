//! 文本注入：trait Injector + 后备链（06 §7.5）。
pub mod paste;
pub mod type_direct;
#[cfg(target_os = "windows")]
pub mod windows;

use crate::error::Result;
use crate::platform::focus::FocusTarget;
use crate::settings::schema::InjectMethod;
use std::sync::atomic::{AtomicU8, Ordering};

const INJECTION_PENDING: u8 = 0;
const INJECTION_CANCELLED: u8 = 1;
const INJECTION_COMMITTED: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionState {
    Pending,
    Cancelled,
    Committed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectionOutcome {
    Injected,
    Cancelled,
}

/// Session injection commit latch. Only one of cancel/commit can win from Pending.
pub struct InjectionLatch(AtomicU8);

impl InjectionLatch {
    pub fn new() -> Self {
        Self(AtomicU8::new(INJECTION_PENDING))
    }

    pub fn cancel(&self) -> bool {
        self.0
            .compare_exchange(
                INJECTION_PENDING,
                INJECTION_CANCELLED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub fn commit(&self) -> bool {
        self.0
            .compare_exchange(
                INJECTION_PENDING,
                INJECTION_COMMITTED,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub fn state(&self) -> InjectionState {
        match self.0.load(Ordering::Acquire) {
            INJECTION_CANCELLED => InjectionState::Cancelled,
            INJECTION_COMMITTED => InjectionState::Committed,
            _ => InjectionState::Pending,
        }
    }
}

impl Default for InjectionLatch {
    fn default() -> Self {
        Self::new()
    }
}

pub fn copy_text_to_clipboard(text: &str) -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        windows::replace_clipboard_text(text)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut clipboard = arboard::Clipboard::new().map_err(|error| {
            crate::error::TypexError::new(
                crate::error::ErrorCode::Internal,
                format!("剪贴板不可用: {error}"),
            )
        })?;
        clipboard.set_text(text).map_err(|error| {
            crate::error::TypexError::new(
                crate::error::ErrorCode::Internal,
                format!("写剪贴板失败: {error}"),
            )
        })
    }
}

pub trait Injector: Send + Sync {
    /// 把文本注入当前焦点位置。
    fn inject(&self, text: &str) -> Result<()>;
    /// Session-aware injection can revalidate an opaque native target immediately before IO.
    fn inject_targeted(&self, text: &str, target: Option<&FocusTarget>) -> Result<()> {
        if target.is_some_and(|target| !target.is_current()) {
            return Err(crate::error::TypexError::new(
                crate::error::ErrorCode::NoFocus,
                "foreground target changed before injection",
            ));
        }
        self.inject(text)
    }
    /// Session-aware entry. Backends with preparation work should override this and commit at the
    /// last boundary before their first OS input call.
    fn inject_targeted_cancellable(
        &self,
        text: &str,
        target: Option<&FocusTarget>,
        latch: &InjectionLatch,
    ) -> Result<InjectionOutcome> {
        if target.is_some_and(|target| !target.is_current()) {
            return Err(crate::error::TypexError::new(
                crate::error::ErrorCode::NoFocus,
                "foreground target changed before injection",
            ));
        }
        if !latch.commit() {
            return Ok(InjectionOutcome::Cancelled);
        }
        self.inject_targeted(text, target)
            .map(|()| InjectionOutcome::Injected)
    }
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

    /// 平台默认链（06 §7.5）：paste 主路径 + type_direct 后备。
    pub fn platform_default(paste_delay_ms: u64) -> Self {
        Self::new(vec![
            Box::new(paste::PasteInjector::new(paste_delay_ms)),
            Box::new(type_direct::TypeDirectInjector),
        ])
    }

    pub fn inject(&self, text: &str) -> Result<()> {
        self.inject_ordered(text, None, None)
    }

    /// 按设置选首选后端：首选排最前，其余保持默认序作后备。
    pub fn inject_with(&self, text: &str, method: InjectMethod) -> Result<()> {
        let preferred = match method {
            InjectMethod::Auto => None,
            InjectMethod::Paste => Some("paste"),
            InjectMethod::TypeDirect => Some("type_direct"),
        };
        self.inject_ordered(text, preferred, None)
    }

    pub fn inject_with_target(
        &self,
        text: &str,
        method: InjectMethod,
        target: Option<&FocusTarget>,
    ) -> Result<()> {
        let preferred = match method {
            InjectMethod::Auto => None,
            InjectMethod::Paste => Some("paste"),
            InjectMethod::TypeDirect => Some("type_direct"),
        };
        self.inject_ordered(text, preferred, target)
    }

    pub fn inject_with_target_cancellable(
        &self,
        text: &str,
        method: InjectMethod,
        target: Option<&FocusTarget>,
        latch: &InjectionLatch,
    ) -> Result<InjectionOutcome> {
        let preferred = match method {
            InjectMethod::Auto => None,
            InjectMethod::Paste => Some("paste"),
            InjectMethod::TypeDirect => Some("type_direct"),
        };
        let mut last_err = None;
        let ordered = self
            .backends
            .iter()
            .filter(|backend| Some(backend.name()) == preferred)
            .chain(
                self.backends
                    .iter()
                    .filter(|backend| Some(backend.name()) != preferred),
            );
        for backend in ordered {
            match backend.inject_targeted_cancellable(text, target, latch) {
                Ok(outcome) => return Ok(outcome),
                Err(error) => {
                    if latch.state() == InjectionState::Committed
                        || matches!(
                            error.code,
                            crate::error::ErrorCode::NoFocus
                                | crate::error::ErrorCode::InjectionBlocked
                        )
                    {
                        return Err(error);
                    }
                    tracing::warn!(
                        "注入后端 {} 失败: {}，尝试下一个",
                        backend.name(),
                        error.message
                    );
                    last_err = Some(error);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            crate::error::TypexError::new(crate::error::ErrorCode::Internal, "无可用注入后端")
        }))
    }

    fn inject_ordered(
        &self,
        text: &str,
        preferred: Option<&str>,
        target: Option<&FocusTarget>,
    ) -> Result<()> {
        let mut last_err = None;
        let ordered = self
            .backends
            .iter()
            .filter(|b| Some(b.name()) == preferred)
            .chain(self.backends.iter().filter(|b| Some(b.name()) != preferred));
        for backend in ordered {
            match backend.inject_targeted(text, target) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if matches!(
                        e.code,
                        crate::error::ErrorCode::NoFocus
                            | crate::error::ErrorCode::InjectionBlocked
                    ) {
                        tracing::warn!(
                            "注入后端 {} 失败: {}，终止后备链",
                            backend.name(),
                            e.message
                        );
                        return Err(e);
                    }
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

    /// 具名 mock：验证 inject_with 的首选排序。
    struct NamedInjector {
        name: &'static str,
        fail: bool,
        calls: Arc<AtomicU32>,
    }

    impl Injector for NamedInjector {
        fn inject(&self, _text: &str) -> Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err(TypexError::new(ErrorCode::Internal, "mock fail"))
            } else {
                Ok(())
            }
        }
        fn name(&self) -> &'static str {
            self.name
        }
    }

    struct PolicyBlockedInjector {
        calls: Arc<AtomicU32>,
    }

    impl Injector for PolicyBlockedInjector {
        fn inject(&self, _text: &str) -> Result<()> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Err(TypexError::new(
                ErrorCode::InjectionBlocked,
                "mock policy block",
            ))
        }

        fn name(&self) -> &'static str {
            "policy_blocked"
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
    fn chain_does_not_retry_indeterminate_or_policy_blocked_input() {
        let blocked = Arc::new(AtomicU32::new(0));
        let fallback = Arc::new(AtomicU32::new(0));
        let chain = InjectorChain::new(vec![
            Box::new(PolicyBlockedInjector {
                calls: blocked.clone(),
            }),
            Box::new(MockInjector {
                fail: false,
                calls: fallback.clone(),
            }),
        ]);

        let error = chain.inject("hi").unwrap_err();
        assert_eq!(error.code, ErrorCode::InjectionBlocked);
        assert_eq!(blocked.load(Ordering::SeqCst), 1);
        assert_eq!(fallback.load(Ordering::SeqCst), 0);
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

    #[test]
    fn inject_with_prefers_selected_backend() {
        let paste = Arc::new(AtomicU32::new(0));
        let typed = Arc::new(AtomicU32::new(0));
        let chain = InjectorChain::new(vec![
            Box::new(NamedInjector {
                name: "paste",
                fail: false,
                calls: paste.clone(),
            }),
            Box::new(NamedInjector {
                name: "type_direct",
                fail: false,
                calls: typed.clone(),
            }),
        ]);
        chain.inject_with("hi", InjectMethod::TypeDirect).unwrap();
        assert_eq!(typed.load(Ordering::SeqCst), 1);
        assert_eq!(paste.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn inject_with_falls_back_when_preferred_fails() {
        let paste = Arc::new(AtomicU32::new(0));
        let typed = Arc::new(AtomicU32::new(0));
        let chain = InjectorChain::new(vec![
            Box::new(NamedInjector {
                name: "paste",
                fail: false,
                calls: paste.clone(),
            }),
            Box::new(NamedInjector {
                name: "type_direct",
                fail: true,
                calls: typed.clone(),
            }),
        ]);
        chain.inject_with("hi", InjectMethod::TypeDirect).unwrap();
        assert_eq!(typed.load(Ordering::SeqCst), 1);
        assert_eq!(paste.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn inject_with_auto_keeps_default_order() {
        let paste = Arc::new(AtomicU32::new(0));
        let typed = Arc::new(AtomicU32::new(0));
        let chain = InjectorChain::new(vec![
            Box::new(NamedInjector {
                name: "paste",
                fail: false,
                calls: paste.clone(),
            }),
            Box::new(NamedInjector {
                name: "type_direct",
                fail: false,
                calls: typed.clone(),
            }),
        ]);
        chain.inject_with("hi", InjectMethod::Auto).unwrap();
        assert_eq!(paste.load(Ordering::SeqCst), 1);
        assert_eq!(typed.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn cancellation_wins_pending_latch_and_skips_every_backend() {
        let first = Arc::new(AtomicU32::new(0));
        let second = Arc::new(AtomicU32::new(0));
        let chain = InjectorChain::new(vec![
            Box::new(MockInjector {
                fail: false,
                calls: first.clone(),
            }),
            Box::new(MockInjector {
                fail: false,
                calls: second.clone(),
            }),
        ]);
        let latch = InjectionLatch::new();

        assert!(latch.cancel());
        assert_eq!(
            chain
                .inject_with_target_cancellable("hi", InjectMethod::Auto, None, &latch)
                .unwrap(),
            InjectionOutcome::Cancelled
        );
        assert_eq!(first.load(Ordering::SeqCst), 0);
        assert_eq!(second.load(Ordering::SeqCst), 0);
        assert_eq!(latch.state(), InjectionState::Cancelled);
    }

    #[test]
    fn commit_wins_latch_and_later_cancel_is_ignored() {
        let calls = Arc::new(AtomicU32::new(0));
        let chain = InjectorChain::new(vec![Box::new(MockInjector {
            fail: false,
            calls: calls.clone(),
        })]);
        let latch = InjectionLatch::new();

        assert_eq!(
            chain
                .inject_with_target_cancellable("hi", InjectMethod::Auto, None, &latch)
                .unwrap(),
            InjectionOutcome::Injected
        );
        assert!(!latch.cancel());
        assert_eq!(latch.state(), InjectionState::Committed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}
