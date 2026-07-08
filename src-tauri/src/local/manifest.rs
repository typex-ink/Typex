//! 本地模型清单（03 §8 / ADR-22）。
//!
//! 模型来源分两类：
//! - 内置清单：随应用更新，可下载；
//! - 用户清单：`{app_data_dir}/models/user-models.json`，由导入功能维护。

use std::path::{Path, PathBuf};

/// 用户导入模型清单文件名，位于 `{app_data_dir}/models/`。
pub const USER_MODELS_FILE: &str = "user-models.json";

/// 模型文件描述。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ModelFile {
    /// 文件名（相对于 `{data_dir}/models/{model_id}/`）。
    pub name: String,
    /// 文件字节数（精确值；0 表示待测量）。
    pub bytes: u64,
    /// SHA-256 十六进制（空串 = 占位，下载时跳过校验）。
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

/// 单个下载源。`url_prefix + "/" + file.name` 得到完整下载 URL。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ModelSource {
    /// 稳定源 key，如 `huggingface` / `modelscope` / `official`。
    pub id: String,
    /// UI 显示名。
    pub label: String,
    /// 文件 URL 前缀。
    pub url_prefix: String,
}

/// 单条模型清单条目。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
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
    /// 许可证标识符（SPDX 或上游模型卡标识）。
    pub license: String,
    /// 可下载源。用户导入模型为空。
    #[serde(default)]
    pub sources: Vec<ModelSource>,
    /// 最低推荐 RAM（GiB，整数）。
    pub min_ram_gb: u32,
    /// 是否需要 GPU 加速（Metal / CUDA / Vulkan）。
    pub requires_gpu: bool,
}

/// 用户模型 manifest 形态。
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct UserModelManifest {
    pub models: Vec<ModelEntry>,
}

fn hf(url_prefix: &str) -> ModelSource {
    ModelSource {
        id: "huggingface".into(),
        label: "HuggingFace".into(),
        url_prefix: url_prefix.into(),
    }
}

fn ms(url_prefix: &str) -> ModelSource {
    ModelSource {
        id: "modelscope".into(),
        label: "ModelScope".into(),
        url_prefix: url_prefix.into(),
    }
}

/// 模型存储根目录：`{data_dir}/models/{model_id}/`。
pub fn model_dir(data_dir: &Path, model_id: &str) -> PathBuf {
    data_dir.join("models").join(model_id)
}

/// 用户模型 manifest 路径。
pub fn user_manifest_path(data_dir: &Path) -> PathBuf {
    data_dir.join("models").join(USER_MODELS_FILE)
}

/// 返回内置模型条目。
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
                    bytes: 239_233_841,
                    sha256: "".into(),
                },
                ModelFile {
                    name: "tokens.txt".into(),
                    bytes: 315_894,
                    sha256: "".into(),
                },
            ],
            license: "Apache-2.0".into(),
            sources: vec![
                hf(
                    "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main",
                ),
                ms("https://modelscope.cn/models/poloniumrock/SenseVoiceSmallOnnx/resolve/master"),
            ],
            min_ram_gb: 2,
            requires_gpu: false,
        },
        ModelEntry {
            id: "qwen3-asr-0.6b-q8".into(),
            display_name: "Qwen3-ASR 0.6B Q8_0".into(),
            purpose: ModelPurpose::Stt,
            engine: ModelEngine::Llama,
            files: vec![
                ModelFile {
                    name: "Qwen3-ASR-0.6B-Q8_0.gguf".into(),
                    bytes: 804_749_248,
                    sha256: "".into(),
                },
                ModelFile {
                    name: "mmproj-Qwen3-ASR-0.6B-Q8_0.gguf".into(),
                    bytes: 214_392_480,
                    sha256: "".into(),
                },
            ],
            license: "Apache-2.0".into(),
            sources: vec![
                hf("https://huggingface.co/ggml-org/Qwen3-ASR-0.6B-GGUF/resolve/main"),
                ms("https://modelscope.cn/models/ggml-org/Qwen3-ASR-0.6B-GGUF/resolve/master"),
            ],
            min_ram_gb: 4,
            requires_gpu: false,
        },
        ModelEntry {
            id: "qwen3-asr-1.7b-q8".into(),
            display_name: "Qwen3-ASR 1.7B Q8_0".into(),
            purpose: ModelPurpose::Stt,
            engine: ModelEngine::Llama,
            files: vec![
                ModelFile {
                    name: "Qwen3-ASR-1.7B-Q8_0.gguf".into(),
                    bytes: 2_165_034_944,
                    sha256: "".into(),
                },
                ModelFile {
                    name: "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf".into(),
                    bytes: 355_709_344,
                    sha256: "".into(),
                },
            ],
            license: "Apache-2.0".into(),
            sources: vec![
                hf("https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/main"),
                ms("https://modelscope.cn/models/ggml-org/Qwen3-ASR-1.7B-GGUF/resolve/master"),
            ],
            min_ram_gb: 8,
            requires_gpu: true,
        },
        // ── LLM ───────────────────────────────────────────────────────────────
        ModelEntry {
            id: "qwen3.5-0.8b-q4".into(),
            display_name: "Qwen3.5 0.8B Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "Qwen3.5-0.8B-Q4_K_M.gguf".into(),
                bytes: 532_517_120,
                sha256: "".into(),
            }],
            license: "Apache-2.0".into(),
            sources: vec![
                hf("https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF/resolve/main"),
                ms("https://modelscope.cn/models/unsloth/Qwen3.5-0.8B-GGUF/resolve/master"),
            ],
            min_ram_gb: 4,
            requires_gpu: false,
        },
        ModelEntry {
            id: "qwen3.5-2b-q4".into(),
            display_name: "Qwen3.5 2B Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "Qwen3.5-2B-Q4_K_M.gguf".into(),
                bytes: 1_280_835_840,
                sha256: "".into(),
            }],
            license: "Apache-2.0".into(),
            sources: vec![
                hf("https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/main"),
                ms("https://modelscope.cn/models/unsloth/Qwen3.5-2B-GGUF/resolve/master"),
            ],
            min_ram_gb: 8,
            requires_gpu: false,
        },
        ModelEntry {
            id: "qwen3.5-4b-q4".into(),
            display_name: "Qwen3.5 4B Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "Qwen3.5-4B-Q4_K_M.gguf".into(),
                bytes: 2_740_937_888,
                sha256: "".into(),
            }],
            license: "Apache-2.0".into(),
            sources: vec![
                hf("https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/main"),
                ms("https://modelscope.cn/models/unsloth/Qwen3.5-4B-GGUF/resolve/master"),
            ],
            min_ram_gb: 16,
            requires_gpu: true,
        },
        ModelEntry {
            id: "qwen3.5-9b-q4".into(),
            display_name: "Qwen3.5 9B Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "Qwen3.5-9B-Q4_K_M.gguf".into(),
                bytes: 5_520_000_000,
                sha256: "".into(),
            }],
            license: "Apache-2.0".into(),
            sources: vec![
                hf("https://huggingface.co/unsloth/Qwen3.5-9B-GGUF/resolve/main"),
                ms("https://modelscope.cn/models/unsloth/Qwen3.5-9B-GGUF/resolve/master"),
            ],
            min_ram_gb: 24,
            requires_gpu: true,
        },
        ModelEntry {
            id: "smollm3-3b-q4".into(),
            display_name: "SmolLM3 3B Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "SmolLM3-3B-Q4_K_M.gguf".into(),
                bytes: 1_920_000_000,
                sha256: "".into(),
            }],
            license: "Apache-2.0".into(),
            sources: vec![hf(
                "https://huggingface.co/ggml-org/SmolLM3-3B-GGUF/resolve/main",
            )],
            min_ram_gb: 8,
            requires_gpu: false,
        },
        ModelEntry {
            id: "granite-3.3-2b-instruct-q4".into(),
            display_name: "Granite 3.3 2B Instruct Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "granite-3.3-2b-instruct-Q4_K_M.gguf".into(),
                bytes: 1_340_000_000,
                sha256: "".into(),
            }],
            license: "Apache-2.0".into(),
            sources: vec![hf(
                "https://huggingface.co/ibm-granite/granite-3.3-2b-instruct-GGUF/resolve/main",
            )],
            min_ram_gb: 8,
            requires_gpu: false,
        },
        ModelEntry {
            id: "phi-4-mini-instruct-q4".into(),
            display_name: "Phi-4 Mini Instruct Q4".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "Phi-4-mini-instruct-Q4_K_M.gguf".into(),
                bytes: 2_490_000_000,
                sha256: "".into(),
            }],
            license: "MIT".into(),
            sources: vec![hf(
                "https://huggingface.co/microsoft/Phi-4-mini-instruct-gguf/resolve/main",
            )],
            min_ram_gb: 8,
            requires_gpu: false,
        },
    ]
}

/// 读取用户导入模型清单；损坏时返回空列表并保留原文件。
pub fn load_user_catalog(data_dir: &Path) -> Vec<ModelEntry> {
    let path = user_manifest_path(data_dir);
    match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<UserModelManifest>(&text) {
            Ok(manifest) => manifest.models,
            Err(e) => {
                tracing::warn!(path = %path.display(), err = %e, "用户模型清单解析失败");
                Vec::new()
            }
        },
        Err(_) => Vec::new(),
    }
}

/// 保存用户导入模型清单。
pub fn save_user_catalog(data_dir: &Path, models: &[ModelEntry]) -> std::io::Result<()> {
    let path = user_manifest_path(data_dir);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let manifest = UserModelManifest {
        models: models.to_vec(),
    };
    let json = serde_json::to_string_pretty(&manifest).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// 内置清单 + 用户导入清单。若 ID 冲突，内置清单优先。
pub fn catalog_with_imported(data_dir: &Path) -> Vec<ModelEntry> {
    let mut all = catalog();
    for imported in load_user_catalog(data_dir) {
        if !all.iter().any(|entry| entry.id == imported.id) {
            all.push(imported);
        }
    }
    all
}

/// 查找模型，并返回 `is_imported`。
pub fn find_model(data_dir: &Path, model_id: &str) -> Option<(ModelEntry, bool)> {
    if let Some(entry) = catalog().into_iter().find(|entry| entry.id == model_id) {
        return Some((entry, false));
    }
    load_user_catalog(data_dir)
        .into_iter()
        .find(|entry| entry.id == model_id)
        .map(|entry| (entry, true))
}

pub fn append_user_model(data_dir: &Path, entry: ModelEntry) -> std::io::Result<()> {
    let mut models = load_user_catalog(data_dir);
    models.retain(|model| model.id != entry.id);
    models.push(entry);
    save_user_catalog(data_dir, &models)
}

/// 删除用户清单中的模型条目；返回是否删除了条目。
pub fn remove_user_model(data_dir: &Path, model_id: &str) -> std::io::Result<bool> {
    let mut models = load_user_catalog(data_dir);
    let before = models.len();
    models.retain(|model| model.id != model_id);
    save_user_catalog(data_dir, &models)?;
    Ok(models.len() != before)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_by_id(id: &str) -> ModelEntry {
        catalog()
            .into_iter()
            .find(|entry| entry.id == id)
            .unwrap_or_else(|| panic!("缺少模型条目：{id}"))
    }

    fn file_names(entry: &ModelEntry) -> Vec<String> {
        entry.files.iter().map(|file| file.name.clone()).collect()
    }

    #[test]
    fn catalog_has_expanded_entries() {
        assert!(catalog().len() >= 10);
    }

    #[test]
    fn all_entries_have_non_empty_id_and_files() {
        for entry in catalog() {
            assert!(!entry.id.is_empty(), "id 为空：{entry:?}");
            assert!(!entry.files.is_empty(), "文件列表为空：{}", entry.id);
            assert!(
                !entry.sources.is_empty(),
                "内置模型必须至少有一个下载源：{}",
                entry.id
            );
        }
    }

    #[test]
    fn gpu_required_entries_are_correct() {
        let gpu_required: Vec<_> = catalog()
            .iter()
            .filter(|e| e.requires_gpu)
            .map(|e| e.id.clone())
            .collect();
        assert!(gpu_required.contains(&"qwen3-asr-1.7b-q8".to_string()));
        assert!(gpu_required.contains(&"qwen3.5-4b-q4".to_string()));
        assert!(gpu_required.contains(&"qwen3.5-9b-q4".to_string()));
    }

    #[test]
    fn qwen_asr_entries_include_mmproj() {
        for id in ["qwen3-asr-0.6b-q8", "qwen3-asr-1.7b-q8"] {
            let names = file_names(&entry_by_id(id));
            assert!(names.iter().any(|name| name.starts_with("mmproj")));
        }
    }

    #[test]
    fn imported_catalog_is_merged_after_builtins() {
        let dir = tempfile::tempdir().unwrap();
        let imported = ModelEntry {
            id: "user-demo".into(),
            display_name: "User Demo".into(),
            purpose: ModelPurpose::Llm,
            engine: ModelEngine::Llama,
            files: vec![ModelFile {
                name: "demo.gguf".into(),
                bytes: 10,
                sha256: String::new(),
            }],
            license: "unknown".into(),
            sources: Vec::new(),
            min_ram_gb: 4,
            requires_gpu: false,
        };
        append_user_model(dir.path(), imported).unwrap();
        let merged = catalog_with_imported(dir.path());
        assert!(merged.iter().any(|entry| entry.id == "user-demo"));
        assert_eq!(merged[0].id, "sense-voice-small-int8");
    }
}
