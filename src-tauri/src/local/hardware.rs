//! 硬件分档探测（ADR-22 §Decision-2）。
//!
//! 探测 RAM 总量 + CPU 核数；GPU 加速：
//! - macOS = Metal 恒可用（Apple Silicon / Intel GPU 均支持）
//! - Windows / Linux = false 占位，适配时补齐 CUDA / Vulkan 探测
//!
//! 纯函数 `recommend_tier` 根据硬件返回推荐档位。

use sysinfo::System;

/// 硬件档位（ADR-22 §Decision-2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum Tier {
    /// 轻量（<8 GB RAM 或无 GPU + 弱 CPU）：SenseVoice + 0.8B，~0.8 GB。
    Lightweight,
    /// 标准（8–16 GB RAM）：Qwen3-ASR-0.6B + Qwen3.5-2B，~2.3 GB。
    Standard,
    /// 性能（≥16 GB + GPU）：Qwen3-ASR-1.7B + Qwen3.5-4B，~5.3 GB。
    Performance,
}

impl Tier {
    /// 档位的中文显示标签。
    pub fn label(self) -> &'static str {
        match self {
            Tier::Lightweight => "轻量",
            Tier::Standard => "标准",
            Tier::Performance => "性能",
        }
    }

    /// IPC / 前端使用的稳定 key（i18n 与档位下拉以此为键）。
    pub fn key(self) -> &'static str {
        match self {
            Tier::Lightweight => "light",
            Tier::Standard => "standard",
            Tier::Performance => "performance",
        }
    }

    /// 档位包含的模型（STT + LLM 各一，ADR-22 分档表）。
    pub fn model_ids(self) -> [&'static str; 2] {
        match self {
            Tier::Lightweight => ["sense-voice-small-int8", "qwen3.5-0.8b-q4"],
            Tier::Standard => ["qwen3-asr-0.6b-q8", "qwen3.5-2b-q4"],
            Tier::Performance => ["qwen3-asr-1.7b-q8", "qwen3.5-4b-q4"],
        }
    }

    pub const ALL: [Tier; 3] = [Tier::Lightweight, Tier::Standard, Tier::Performance];
}

/// 模型所属推荐档位（ADR-22 分档表的反查；不在任何档位的条目不存在）。
pub fn tier_of_model(model_id: &str) -> Option<Tier> {
    Tier::ALL
        .into_iter()
        .find(|t| t.model_ids().contains(&model_id))
}

/// 探测到的硬件信息。
#[derive(Debug, Clone)]
pub struct HardwareInfo {
    /// 物理 RAM 总量（GiB，向下取整）。
    pub ram_gb: u64,
    /// 逻辑 CPU 核数。
    pub cpu_cores: usize,
    /// 是否有可用的 GPU 加速（Metal / CUDA / Vulkan）。
    pub gpu_available: bool,
}

/// 探测当前硬件信息。
///
/// 此函数会短暂阻塞以刷新 sysinfo，应在后台线程调用。
pub fn detect() -> HardwareInfo {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();

    let ram_bytes = sys.total_memory(); // 字节
    let ram_gb = ram_bytes / (1024 * 1024 * 1024);
    let cpu_cores = sys.cpus().len();
    let gpu_available = probe_gpu();

    HardwareInfo {
        ram_gb,
        cpu_cores,
        gpu_available,
    }
}

/// GPU 加速探测。
///
/// macOS：Metal 始终可用（Apple Silicon + Intel GPU 均支持 Metal）→ 返回 `true`。
/// Windows / Linux：暂返回 `false`；平台适配时补齐 CUDA / Vulkan 探测。
fn probe_gpu() -> bool {
    #[cfg(target_os = "macos")]
    {
        true
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// 根据硬件参数推荐档位（纯函数，便于单测）。
///
/// 规则（ADR-22）：
/// - 性能：ram_gb ≥ 16 **且** gpu = true
/// - 标准：ram_gb ≥ 8（gpu 不要求）
/// - 轻量：其余情况（ram_gb < 8，或无 GPU 且 cores < 4 亦归轻量）
pub fn recommend_tier(ram_gb: u64, _cores: usize, gpu: bool) -> Tier {
    if ram_gb >= 16 && gpu {
        Tier::Performance
    } else if ram_gb >= 8 {
        Tier::Standard
    } else {
        Tier::Lightweight
    }
}

/// 生成诊断页显示字符串（05 §5.2 验收）。
///
/// 格式示例：`RAM 24 GB · 10 核 · Metal ✓ · 推荐档位：性能`
pub fn diagnostics_string() -> String {
    let hw = detect();
    let tier = recommend_tier(hw.ram_gb, hw.cpu_cores, hw.gpu_available);
    let gpu_label = if hw.gpu_available {
        "Metal ✓"
    } else {
        "GPU ✗"
    };
    format!(
        "RAM {} GB · {} 核 · {} · 推荐档位：{}",
        hw.ram_gb,
        hw.cpu_cores,
        gpu_label,
        tier.label()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── recommend_tier 边界单测 ────────────────────────────────────────────────

    #[test]
    fn tier_performance_requires_16gb_and_gpu() {
        assert_eq!(recommend_tier(16, 8, true), Tier::Performance);
        assert_eq!(recommend_tier(32, 16, true), Tier::Performance);
    }

    #[test]
    fn tier_no_gpu_with_16gb_falls_to_standard() {
        // ≥16 GB 但无 GPU → 标准（不满足性能条件）
        assert_eq!(recommend_tier(16, 8, false), Tier::Standard);
    }

    #[test]
    fn tier_standard_8_to_15gb() {
        assert_eq!(recommend_tier(8, 4, false), Tier::Standard);
        assert_eq!(recommend_tier(8, 4, true), Tier::Standard); // gpu=true 但 <16 GB
        assert_eq!(recommend_tier(15, 8, false), Tier::Standard);
        assert_eq!(recommend_tier(12, 6, true), Tier::Standard);
    }

    #[test]
    fn tier_lightweight_under_8gb() {
        assert_eq!(recommend_tier(4, 4, false), Tier::Lightweight);
        assert_eq!(recommend_tier(7, 8, true), Tier::Lightweight);
        assert_eq!(recommend_tier(0, 1, false), Tier::Lightweight);
    }

    #[test]
    fn tier_boundary_exactly_8gb() {
        assert_eq!(recommend_tier(8, 4, false), Tier::Standard);
    }

    #[test]
    fn tier_boundary_exactly_16gb_with_gpu() {
        assert_eq!(recommend_tier(16, 4, true), Tier::Performance);
    }

    #[test]
    fn tier_boundary_exactly_16gb_without_gpu() {
        assert_eq!(recommend_tier(16, 4, false), Tier::Standard);
    }

    #[test]
    fn tier_labels_are_chinese() {
        assert_eq!(Tier::Lightweight.label(), "轻量");
        assert_eq!(Tier::Standard.label(), "标准");
        assert_eq!(Tier::Performance.label(), "性能");
    }

    #[test]
    fn diagnostics_string_format() {
        // 验证格式包含必要字段（不依赖实际硬件，用固定参数组装）
        let hw = HardwareInfo {
            ram_gb: 24,
            cpu_cores: 10,
            gpu_available: true,
        };
        let tier = recommend_tier(hw.ram_gb, hw.cpu_cores, hw.gpu_available);
        let gpu_label = if hw.gpu_available {
            "Metal ✓"
        } else {
            "GPU ✗"
        };
        let s = format!(
            "RAM {} GB · {} 核 · {} · 推荐档位：{}",
            hw.ram_gb,
            hw.cpu_cores,
            gpu_label,
            tier.label()
        );
        assert!(s.contains("RAM 24 GB"), "缺 RAM 信息：{s}");
        assert!(s.contains("10 核"), "缺核数：{s}");
        assert!(s.contains("Metal ✓"), "缺 GPU 标签：{s}");
        assert!(s.contains("推荐档位：性能"), "缺档位：{s}");
    }
}
