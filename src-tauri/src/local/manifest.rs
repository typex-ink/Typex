//! 内置模型清单（03 §8 / ADR-22）。
//!
//! 每条目包含：id、显示名、用途、推理引擎、文件列表（文件名+字节数+SHA-256）、
//! 许可证、双源 URL（HuggingFace + ModelScope）、最小 RAM、是否需要 GPU 加速。
//! SHA-256 留空串占位——CI 下载验收时填入真实哈希（v1.1 首发前补齐）。

/// 模型文件描述。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ModelFile {
    /// 文件名（相对于 `{data_dir}/models/{model_id}/`）。
    pub name: String,
    /// 文件字节数（精确值；0 表示待测量）。
    pub bytes: u64,
    /// SHA-256 十六进制（空串 = 占位，待补齐）。
    pub sha256: String,
}

/// 用途分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ModelPurpose {
    /// 语音转写（Speech-to-Text）。
    Stt,
    /// 文本润色 / 翻译（Large Language Model）。
    Llm,
}

/// 推理引擎。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum ModelEngine {
    /// sherpa-onnx（SenseVoice 轻量 STT）。
    Sherpa,
    /// llama.cpp（Qwen3-ASR / Qwen3.5 GGUF）。
    Llama,
}

/// 双源 URL（HuggingFace 主源 + ModelScope 镜像）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ModelSources {
    /// HuggingFace 文件 URL 前缀（拼 `/{file.name}` 得到完整 URL）。
    pub huggingface: String,
    /// ModelScope 文件 URL 前缀。
    pub modelscope: String,
}

/// 单条模型清单条目。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ModelEntry {
    /// 唯一 ID（用作存储子目录名）。
    pub id: String,
    /// 显示名（设置页 / 下载对话框）。
    pub display_name: String,
    /// 用途。
    pub purpose: ModelPurpose,
    /// 推理引擎。
    pub engine: ModelEngine,
    /// 模型文件列表。
    pub files: Vec<ModelFile>,
    /// 许可证标识符（SPDX）。
    pub license: String,
    /// 双源 URL。
    pub sources: ModelSources,
    /// 最低推荐 RAM（GiB，整数）。
    pub min_ram_gb: u32,
    /// 是否需要 GPU 加速（Metal / CUDA / Vulkan）。
    pub requires_gpu: bool,
}

/// 返回全部内置模型条目（ADR-22 v1.1 初始 6 条）。
pub fn catalog() -> Vec<ModelEntry> {
    vec![
        // ── STT ───────────────────────────────────────────────────────────────
        ModelEntry {
            id: "sense-voice-small-int8".into(),
            display_name: "SenseVoice Small (int8)".into(),
            purpose: ModelPurpose::Stt,
            engine: ModelEngine::Sherpa,
            files: vec![
                ModelFile {
                    name: "model.int8.onnx".into(),
                    bytes: 241_172_480, // ~230 MB
                    sha256: "".into(),  // TODO: 填入真实哈希
                },
                ModelFile {
                    name: "tokens.txt".into(),
                    bytes: 399_360, // ~390 KB
                    sha256: "".into(),
                },
            ],
            license: "Apache-2.0".into(),
            sources: ModelSources {
                huggingface: "https://huggingface.co/FunAudioLLM/SenseVoiceSmall/resolve/main"
                    .into(),
                modelscope: "https://modelscope.cn/models/iic/SenseVoiceSmall/resolve/master"
                    .into(),
            },
            min_ram_gb: 2,
            requires_gpu: false,
        },
        ModelEntry {
            id: "qwen3-asr-0.6b-q4".into(),
            display_name: "Qwen3-ASR 0.6B Q4".into(),
            purpose: ModelPurpose::Stt,
            engine: ModelEngine::Llama,
            files: vec![
                ModelFile {
                    name: "qwen3-asr-0.6b-q4_k_m.gguf".into(),
                    bytes: 536_870_912, // ~0.5 GB
                    sha256: "".into(),
                },
                ModelFile {
                    name: "mmproj-qwen3-asr-0.6b-f16.gguf".into(),
                    bytes: 692_060_160, // ~660 MB（音频编码器投影，CP-8.4）
                    sha256: "".into(),
                },
            ],
            license: "Apache-2.0".into(),
            sources: ModelSources {
                huggingface: "https://huggingface.co/Qwen/Qwen3-ASR-0.6B-GGUF/resolve/main".into(),
                modelscope: "https://modelscope.cn/models/Qwen/Qwen3-ASR-0.6B-GGUF/resolve/master"
                    .into(),
            },
            min_ram_gb: 4,
            requires_gpu: false,
        },
        ModelEntry {
            id: "qwen3-asr-1.7b-q4".into(),
            display_name: "Qwen3-ASR 1.7B Q4".into(),
            purpose: ModelPurpose::Stt,
            engine: ModelEngine::Llama,
            files: vec![
                ModelFile {
                    name: "qwen3-asr-1.7b-q4_k_m.gguf".into(),
                    bytes: 1_181_116_006, // ~1.1 GB
                    sha256: "".into(),
                },
                ModelFile {
                    name: "mmproj-qwen3-asr-1.7b-f16.gguf".into(),
                    bytes: 692_060_160, // ~660 MB（音频编码器投影，CP-8.4）
                    sha256: "".into(),
                },
            ],
            license: "Apache-2.0".into(),
            sources: ModelSources {
                huggingface: "https://huggingface.co/Qwen/Qwen3-ASR-1.7B-GGUF/resolve/main".into(),
                modelscope: "https://modelscope.cn/models/Qwen/Qwen3-ASR-1.7B-GGUF/resolve/master"
                    .into(),
            },
            min_ram_gb: 8,
            requires_gpu: true, // 纯 CPU 低于实时（RTFx≈0.49），需 GPU 加速（ADR-22）
        },
        // ── LLM ───────────────────────────────────────────────────────────────
        ModelEntry {
            id: "qwen3.5-0.8b-q4".into(),
            display_name: "Qwen3.5 0.8B Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "qwen3.5-0.8b-q4_k_m.gguf".into(),
                bytes: 644_245_094, // ~0.6 GB
                sha256: "".into(),
            }],
            license: "Apache-2.0".into(),
            sources: ModelSources {
                huggingface: "https://huggingface.co/Qwen/Qwen3.5-0.8B-GGUF/resolve/main".into(),
                modelscope: "https://modelscope.cn/models/Qwen/Qwen3.5-0.8B-GGUF/resolve/master"
                    .into(),
            },
            min_ram_gb: 4,
            requires_gpu: false,
        },
        ModelEntry {
            id: "qwen3.5-2b-q4".into(),
            display_name: "Qwen3.5 2B Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "qwen3.5-2b-q4_k_m.gguf".into(),
                bytes: 1_395_864_371, // ~1.3 GB
                sha256: "".into(),
            }],
            license: "Apache-2.0".into(),
            sources: ModelSources {
                huggingface: "https://huggingface.co/Qwen/Qwen3.5-2B-GGUF/resolve/main".into(),
                modelscope: "https://modelscope.cn/models/Qwen/Qwen3.5-2B-GGUF/resolve/master"
                    .into(),
            },
            min_ram_gb: 8,
            requires_gpu: false,
        },
        ModelEntry {
            id: "qwen3.5-4b-q4".into(),
            display_name: "Qwen3.5 4B Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "qwen3.5-4b-q4_k_m.gguf".into(),
                bytes: 2_684_354_560, // ~2.5 GB
                sha256: "".into(),
            }],
            license: "Apache-2.0".into(),
            sources: ModelSources {
                huggingface: "https://huggingface.co/Qwen/Qwen3.5-4B-GGUF/resolve/main".into(),
                modelscope: "https://modelscope.cn/models/Qwen/Qwen3.5-4B-GGUF/resolve/master"
                    .into(),
            },
            min_ram_gb: 16,
            requires_gpu: true,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_six_entries() {
        assert_eq!(catalog().len(), 6);
    }

    #[test]
    fn all_entries_have_non_empty_id_and_files() {
        for entry in catalog() {
            assert!(!entry.id.is_empty(), "id 为空：{entry:?}");
            assert!(!entry.files.is_empty(), "文件列表为空：{}", entry.id);
        }
    }

    #[test]
    fn gpu_required_entries_are_correct() {
        let gpu_required: Vec<_> = catalog()
            .iter()
            .filter(|e| e.requires_gpu)
            .map(|e| e.id.clone())
            .collect();
        // ADR-22：1.7B STT 和 4B LLM 需要 GPU
        assert!(gpu_required.contains(&"qwen3-asr-1.7b-q4".to_string()));
        assert!(gpu_required.contains(&"qwen3.5-4b-q4".to_string()));
        assert_eq!(gpu_required.len(), 2);
    }

    #[test]
    fn all_entries_have_apache_license() {
        for entry in catalog() {
            assert_eq!(entry.license, "Apache-2.0", "许可证不符：{}", entry.id);
        }
    }
}
