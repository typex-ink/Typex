//! 托盘图标状态动画（04 §2.4 / CP-6.9）。
//!
//! 运行时把五柱波形渲染成 RGBA 帧（22×22）喂给托盘：
//! 空闲=静态五柱；录音=实时电平（8 fps）；处理=从左到右依次呼吸；
//! 暂停=40% 透明 + 斜杠；错误=静态五柱 + 红点角标（红点帧非 template）。

use crate::types::{SessionPhase, SessionSnapshot};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use tauri::{AppHandle, Manager, Runtime};

const SIZE: usize = 22;
/// 柱宽 2px、间隔 2px，五柱共 18px，水平居中
const BAR_W: usize = 2;
const GAP: usize = 2;
/// 静态五柱高度（对称山形）
const IDLE_HEIGHTS: [usize; 5] = [6, 10, 14, 10, 6];
/// 录音红 / 错误红（04 §3 唯一彩色）
const RED: [u8; 4] = [226, 61, 45, 255];

/// 托盘视觉状态（snapshot/level 回调写入，动画 task 读取）。
#[derive(Default)]
pub struct TrayVisual {
    /// 0=idle 1=recording 2=processing 3=error
    mode: AtomicU8,
    paused: AtomicBool,
    levels: Mutex<Vec<f32>>,
}

impl TrayVisual {
    pub fn on_snapshot(&self, snap: &SessionSnapshot) {
        let mode = match snap.phase {
            SessionPhase::Recording => 1,
            SessionPhase::Transcribing | SessionPhase::Processing | SessionPhase::Injecting => 2,
            SessionPhase::Failed => 3,
            _ => 0,
        };
        self.mode.store(mode, Ordering::Relaxed);
    }

    pub fn on_levels(&self, levels: &[f32]) {
        *self.levels.lock().unwrap() = levels.to_vec();
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }
}

/// 一帧 RGBA。template=true 时 macOS 会按系统深浅自动着色（只看 alpha）。
struct Frame {
    rgba: Vec<u8>,
    template: bool,
}

fn blank() -> Vec<u8> {
    vec![0u8; SIZE * SIZE * 4]
}

fn put(buf: &mut [u8], x: usize, y: usize, c: [u8; 4]) {
    if x < SIZE && y < SIZE {
        let i = (y * SIZE + x) * 4;
        buf[i..i + 4].copy_from_slice(&c);
    }
}

/// 画一根竖柱：底对齐，高 h，alpha 0–255。
fn draw_bar(buf: &mut [u8], idx: usize, h: usize, alpha: u8) {
    let x0 = (SIZE - (5 * BAR_W + 4 * GAP)) / 2 + idx * (BAR_W + GAP);
    let base = (SIZE + 14) / 2; // 柱体在 14px 高的带内底对齐，垂直居中
    for dy in 0..h.min(14) {
        for dx in 0..BAR_W {
            put(buf, x0 + dx, base - 1 - dy, [0, 0, 0, alpha]);
        }
    }
}

/// 红点角标（右上 6px）。
fn draw_badge(buf: &mut [u8]) {
    let (cx, cy, r) = (SIZE as i32 - 4, 4i32, 3i32);
    for dy in -r..=r {
        for dx in -r..=r {
            if dx * dx + dy * dy <= r * r {
                put(buf, (cx + dx) as usize, (cy + dy) as usize, RED);
            }
        }
    }
}

/// 斜杠角标（暂停）。
fn draw_slash(buf: &mut [u8]) {
    for i in 0..SIZE {
        put(buf, i, SIZE - 1 - i, [0, 0, 0, 255]);
        if i + 1 < SIZE {
            put(buf, i + 1, SIZE - 1 - i, [0, 0, 0, 255]);
        }
    }
}

fn render(visual: &TrayVisual, tick: u64) -> Frame {
    let mut buf = blank();
    if visual.paused.load(Ordering::Relaxed) {
        for (i, h) in IDLE_HEIGHTS.iter().enumerate() {
            draw_bar(&mut buf, i, *h, 102); // 40%
        }
        draw_slash(&mut buf);
        return Frame {
            rgba: buf,
            template: true,
        };
    }
    match visual.mode.load(Ordering::Relaxed) {
        1 => {
            // 录音：实时电平映射五柱高度（macOS template 无红点，动画本身即状态）
            let levels = visual.levels.lock().unwrap().clone();
            for i in 0..5 {
                let l = levels.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
                let h = 3 + (l * 11.0) as usize;
                draw_bar(&mut buf, i, h, 255);
            }
            Frame {
                rgba: buf,
                template: true,
            }
        }
        2 => {
            // 处理：从左到右依次呼吸（透明度 40%↔100%）
            for (i, h) in IDLE_HEIGHTS.iter().enumerate() {
                let phase = ((tick as f32 * 0.35) - i as f32 * 0.7).sin() * 0.5 + 0.5;
                let alpha = (102.0 + phase * 153.0) as u8;
                draw_bar(&mut buf, i, *h, alpha);
            }
            Frame {
                rgba: buf,
                template: true,
            }
        }
        3 => {
            // 错误：静态五柱 + 红点（需真彩，非 template）
            // template 关闭后柱体固定黑色——深色菜单栏对比度降低是已知妥协
            for (i, h) in IDLE_HEIGHTS.iter().enumerate() {
                draw_bar(&mut buf, i, *h, 255);
            }
            draw_badge(&mut buf);
            Frame {
                rgba: buf,
                template: false,
            }
        }
        _ => {
            for (i, h) in IDLE_HEIGHTS.iter().enumerate() {
                draw_bar(&mut buf, i, *h, 255);
            }
            Frame {
                rgba: buf,
                template: true,
            }
        }
    }
}

/// 启动动画 task：空闲静止不重绘（8 fps 仅在录音/处理时耗电）。
pub fn spawn_animator<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let mut tick: u64 = 0;
        let mut last_static = false; // 上一帧是否为静态态（避免空闲期重复 set_icon）
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(125)).await; // 8 fps
            tick += 1;
            let Some(visual) = app.try_state::<std::sync::Arc<TrayVisual>>() else {
                continue;
            };
            let mode = visual.mode.load(Ordering::Relaxed);
            let paused = visual.paused.load(Ordering::Relaxed);
            let is_static = matches!(mode, 0 | 3) || paused;
            if is_static && last_static {
                continue; // 静态态已渲染过，零活动
            }
            last_static = is_static;
            let frame = render(&visual, tick);
            if let Some(tray) = app.tray_by_id("main") {
                let img = tauri::image::Image::new_owned(frame.rgba, SIZE as u32, SIZE as u32);
                let _ = tray.set_icon_as_template(frame.template);
                let _ = tray.set_icon(Some(img));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_sum(buf: &[u8]) -> u64 {
        buf.chunks(4).map(|p| p[3] as u64).sum()
    }

    #[test]
    fn idle_frame_is_template_with_bars() {
        let v = TrayVisual::default();
        let f = render(&v, 0);
        assert!(f.template);
        assert!(alpha_sum(&f.rgba) > 0, "空帧");
    }

    #[test]
    fn error_frame_has_red_badge_not_template() {
        let v = TrayVisual::default();
        v.mode.store(3, Ordering::Relaxed);
        let f = render(&v, 0);
        assert!(!f.template);
        let has_red = f
            .rgba
            .chunks(4)
            .any(|p| p[0] == RED[0] && p[1] == RED[1] && p[3] == 255);
        assert!(has_red, "缺红点角标");
    }

    #[test]
    fn paused_frame_dimmed_with_slash() {
        let v = TrayVisual::default();
        v.set_paused(true);
        let f = render(&v, 0);
        assert!(f.template);
        // 斜杠是满 alpha，柱体 40%——两种 alpha 都存在
        let alphas: std::collections::HashSet<u8> =
            f.rgba.chunks(4).map(|p| p[3]).filter(|a| *a > 0).collect();
        assert!(alphas.contains(&255) && alphas.contains(&102));
    }

    #[test]
    fn recording_frame_reflects_levels() {
        let v = TrayVisual::default();
        v.mode.store(1, Ordering::Relaxed);
        v.on_levels(&[0.0, 0.0, 0.0, 0.0, 0.0]);
        let quiet = alpha_sum(&render(&v, 0).rgba);
        v.on_levels(&[1.0, 1.0, 1.0, 1.0, 1.0]);
        let loud = alpha_sum(&render(&v, 0).rgba);
        assert!(loud > quiet, "电平应影响柱高");
    }

    #[test]
    fn processing_breathes_over_ticks() {
        let v = TrayVisual::default();
        v.mode.store(2, Ordering::Relaxed);
        let a = render(&v, 0).rgba;
        let b = render(&v, 5).rgba;
        assert_ne!(a, b, "呼吸动画应随 tick 变化");
    }
}
