//! 本地 llama.cpp 推理线程策略。
//!
//! Windows 的 Vulkan backend 初始化与模型加载可能超过 Tokio worker 的默认栈，
//! 因此所有 llama.cpp 入口统一使用显式大栈的专属线程。

use std::io;
use std::thread::{Builder, JoinHandle};

pub(crate) const INFERENCE_THREAD_STACK_BYTES: usize = 64 * 1024 * 1024;

fn inference_thread_stack_bytes() -> usize {
    #[cfg(test)]
    if let Some(mebibytes) = std::env::var("TYPEX_TEST_INFERENCE_STACK_MIB")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=1024).contains(value))
    {
        return mebibytes * 1024 * 1024;
    }
    INFERENCE_THREAD_STACK_BYTES
}

#[cfg(target_os = "windows")]
pub(crate) fn current_thread_stack_bytes() -> usize {
    let mut low = 0usize;
    let mut high = 0usize;
    unsafe {
        windows::Win32::System::Threading::GetCurrentThreadStackLimits(&mut low, &mut high);
    }
    high.saturating_sub(low)
}

pub(crate) fn spawn_inference_thread<T>(
    name: &str,
    task: impl FnOnce() -> T + Send + 'static,
) -> io::Result<JoinHandle<T>>
where
    T: Send + 'static,
{
    Builder::new()
        .name(name.to_owned())
        .stack_size(inference_thread_stack_bytes())
        .spawn(task)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inference_worker_uses_the_shared_name_and_stack_policy() {
        assert_eq!(INFERENCE_THREAD_STACK_BYTES, 64 * 1024 * 1024);
        let worker = spawn_inference_thread("typex-test-inference", || {
            let name = std::thread::current().name().map(str::to_owned);
            #[cfg(target_os = "windows")]
            let stack_bytes = current_thread_stack_bytes();
            #[cfg(not(target_os = "windows"))]
            let stack_bytes = INFERENCE_THREAD_STACK_BYTES;
            (name, stack_bytes)
        })
        .unwrap();

        let (name, stack_bytes) = worker.join().unwrap();
        assert_eq!(name.as_deref(), Some("typex-test-inference"));
        assert_eq!(stack_bytes, INFERENCE_THREAD_STACK_BYTES);
    }
}
