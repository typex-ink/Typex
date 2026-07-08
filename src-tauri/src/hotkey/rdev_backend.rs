//! rdev 独立线程监听（macOS/Windows/X11，06 §7.3）。
//! listen-only：不拦截任何按键；判定在 HotkeyDetector 纯逻辑层。

use super::{HotkeyConfig, HotkeyDetector, HotkeyEvent};
use std::sync::mpsc as std_mpsc;
use std::time::Instant;
use tokio::sync::{mpsc, watch};

/// 启动 rdev 监听线程。语义事件经返回的 receiver 消费。
/// `config_rx`：设置变更时热更新键位。
/// `paused_rx`：托盘「暂停 Typex」时置 true，事件全部丢弃。
pub fn spawn(
    initial: HotkeyConfig,
    mut config_rx: watch::Receiver<HotkeyConfig>,
    paused_rx: watch::Receiver<bool>,
) -> mpsc::UnboundedReceiver<HotkeyEvent> {
    let (tx, rx) = mpsc::unbounded_channel();

    // rdev::listen 是阻塞调用且要求整线程；配置热更新经 std mpsc 转发进回调。
    let (cfg_tx, cfg_rx_std) = std_mpsc::channel::<HotkeyConfig>();
    std::thread::Builder::new()
        .name("typex-hotkey-cfg".into())
        .spawn(move || {
            // watch → std channel 桥接线程（轮询：配置变更不频繁，500ms 足够）
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                match config_rx.has_changed() {
                    Ok(true) => {
                        let cfg = config_rx.borrow_and_update().clone();
                        if cfg_tx.send(cfg).is_err() {
                            break;
                        }
                    }
                    Ok(false) => {}
                    Err(_) => break, // sender 已 drop
                }
            }
        })
        .expect("spawn hotkey cfg thread");

    std::thread::Builder::new()
        .name("typex-hotkey".into())
        .spawn(move || {
            let mut detector = HotkeyDetector::new(initial);
            let epoch = Instant::now();
            let result = rdev::listen(move |event: rdev::Event| {
                // 应用热更新（非阻塞轮询）
                while let Ok(cfg) = cfg_rx_std.try_recv() {
                    detector.set_config(cfg);
                }
                if *paused_rx.borrow() {
                    return;
                }
                let (key, down) = match event.event_type {
                    rdev::EventType::KeyPress(k) => (k, true),
                    rdev::EventType::KeyRelease(k) => (k, false),
                    _ => return,
                };
                let t_ms = epoch.elapsed().as_millis() as u64;
                for ev in detector.on_key(&key_id(key), down, t_ms) {
                    let _ = tx.send(ev);
                }
            });
            if let Err(e) = result {
                // macOS 未授权辅助功能时 rdev 静默无事件或直接失败（平台坑 7.2-1）
                tracing::error!("rdev listen 失败（缺辅助功能权限？）: {e:?}");
            }
        })
        .expect("spawn hotkey thread");

    rx
}

/// rdev::Key → 稳定字符串 ID（settings.json 存储形态；与 HotkeySettings 默认值一致）。
pub fn key_id(key: rdev::Key) -> String {
    format!("{key:?}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn key_id_stable_names() {
        assert_eq!(super::key_id(rdev::Key::MetaRight), "MetaRight");
        assert_eq!(super::key_id(rdev::Key::AltGr), "AltGr");
        assert_eq!(super::key_id(rdev::Key::ControlRight), "ControlRight");
        assert_eq!(super::key_id(rdev::Key::Escape), "Escape");
    }
}
