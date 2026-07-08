//! 用户本地模型导入（03 §8）。
//!
//! 导入采用托管模式：模型文件复制（同盘优先硬链接）到
//! `{app_data_dir}/models/{model_id}/`，再写入用户模型清单。

use crate::local::manifest::{
    self, ModelEngine, ModelEntry, ModelFile, ModelPurpose, append_user_model,
};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("名称不能为空")]
    EmptyName,
    #[error("未选择模型文件")]
    EmptyFiles,
    #[error("不支持的用途：{0}")]
    InvalidPurpose(String),
    #[error("不支持的运行时：{0}")]
    InvalidEngine(String),
    #[error("本地 LLM 仅支持 llama.cpp GGUF")]
    UnsupportedLlmEngine,
    #[error("SenseVoice/sherpa 导入需要 .onnx 模型和 tokens.txt")]
    MissingSherpaFiles,
    #[error("llama ASR 导入需要主 GGUF 和 mmproj GGUF")]
    MissingLlamaAsrFiles,
    #[error("LLM 导入需要且只能选择一个 GGUF 文件")]
    InvalidLlmFiles,
    #[error("文件不存在：{0}")]
    MissingFile(String),
    #[error("文件名重复：{0}")]
    DuplicateFileName(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("序列化错误：{0}")]
    Json(#[from] serde_json::Error),
}

/// 前端传入的导入请求。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ImportLocalModelRequest {
    pub display_name: String,
    /// `stt` | `llm`
    pub purpose: String,
    /// `llama` | `sherpa`
    pub engine: String,
    /// 用户选择的本地文件路径。
    pub files: Vec<String>,
    /// SPDX 或上游模型卡许可证。空值会保存为 `unknown`。
    pub license: String,
    pub min_ram_gb: u32,
    pub requires_gpu: bool,
}

pub fn parse_purpose(value: &str) -> Result<ModelPurpose, ImportError> {
    match value {
        "stt" => Ok(ModelPurpose::Stt),
        "llm" => Ok(ModelPurpose::Llm),
        other => Err(ImportError::InvalidPurpose(other.into())),
    }
}

pub fn parse_engine(value: &str) -> Result<ModelEngine, ImportError> {
    match value {
        "llama" => Ok(ModelEngine::Llama),
        "sherpa" => Ok(ModelEngine::Sherpa),
        other => Err(ImportError::InvalidEngine(other.into())),
    }
}

fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in input.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    let slug = out.trim_matches('-');
    if slug.is_empty() {
        "model".into()
    } else {
        slug.chars().take(40).collect()
    }
}

fn sha256_of_path(path: &Path) -> Result<String, ImportError> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn file_name(path: &Path) -> Result<String, ImportError> {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.trim().is_empty())
        .ok_or_else(|| ImportError::MissingFile(path.display().to_string()))
}

fn validate_files(
    purpose: ModelPurpose,
    engine: ModelEngine,
    paths: &[PathBuf],
) -> Result<(), ImportError> {
    if paths.is_empty() {
        return Err(ImportError::EmptyFiles);
    }
    for path in paths {
        if !path.is_file() {
            return Err(ImportError::MissingFile(path.display().to_string()));
        }
    }

    let names: Vec<String> = paths
        .iter()
        .map(|path| file_name(path).map(|name| name.to_ascii_lowercase()))
        .collect::<Result<_, _>>()?;

    match (purpose, engine) {
        (ModelPurpose::Llm, ModelEngine::Llama) => {
            let gguf = names.iter().filter(|name| name.ends_with(".gguf")).count();
            if gguf != 1 || names.len() != 1 || names.iter().any(|n| n.starts_with("mmproj")) {
                return Err(ImportError::InvalidLlmFiles);
            }
        }
        (ModelPurpose::Llm, ModelEngine::Sherpa) => {
            return Err(ImportError::UnsupportedLlmEngine);
        }
        (ModelPurpose::Stt, ModelEngine::Sherpa) => {
            let has_onnx = names.iter().any(|name| name.ends_with(".onnx"));
            let has_tokens = names.iter().any(|name| name == "tokens.txt");
            if !has_onnx || !has_tokens {
                return Err(ImportError::MissingSherpaFiles);
            }
        }
        (ModelPurpose::Stt, ModelEngine::Llama) => {
            let has_main = names
                .iter()
                .any(|name| name.ends_with(".gguf") && !name.starts_with("mmproj"));
            let has_mmproj = names
                .iter()
                .any(|name| name.starts_with("mmproj") && name.ends_with(".gguf"));
            if !has_main || !has_mmproj {
                return Err(ImportError::MissingLlamaAsrFiles);
            }
        }
    }
    Ok(())
}

fn copy_or_link(src: &Path, dest: &Path) -> Result<(), ImportError> {
    if std::fs::hard_link(src, dest).is_ok() {
        return Ok(());
    }
    std::fs::copy(src, dest)?;
    Ok(())
}

/// 导入模型并返回写入用户清单的条目。
pub fn import_model(
    data_dir: &Path,
    request: ImportLocalModelRequest,
) -> Result<ModelEntry, ImportError> {
    let display_name = request.display_name.trim();
    if display_name.is_empty() {
        return Err(ImportError::EmptyName);
    }
    let purpose = parse_purpose(&request.purpose)?;
    let engine = parse_engine(&request.engine)?;
    let paths: Vec<PathBuf> = request
        .files
        .iter()
        .map(|path| PathBuf::from(path.trim()))
        .filter(|path| !path.as_os_str().is_empty())
        .collect();
    validate_files(purpose, engine, &paths)?;

    let id = format!(
        "user-{}-{}",
        slugify(display_name),
        uuid::Uuid::new_v4().simple()
    );
    let model_dir = manifest::model_dir(data_dir, &id);
    std::fs::create_dir_all(&model_dir)?;

    let mut seen = HashSet::new();
    let mut files = Vec::with_capacity(paths.len());
    let result = (|| {
        for path in &paths {
            let name = file_name(path)?;
            if !seen.insert(name.clone()) {
                return Err(ImportError::DuplicateFileName(name));
            }
            let dest = model_dir.join(&name);
            copy_or_link(path, &dest)?;
            let bytes = std::fs::metadata(&dest)?.len();
            let sha256 = sha256_of_path(&dest)?;
            files.push(ModelFile {
                name,
                bytes,
                sha256,
            });
        }
        Ok::<_, ImportError>(())
    })();

    if let Err(err) = result {
        let _ = std::fs::remove_dir_all(&model_dir);
        return Err(err);
    }

    let license = request.license.trim();
    let entry = ModelEntry {
        id,
        display_name: display_name.into(),
        purpose,
        engine,
        files,
        license: if license.is_empty() {
            "unknown".into()
        } else {
            license.into()
        },
        sources: Vec::new(),
        min_ram_gb: request.min_ram_gb.max(1),
        requires_gpu: request.requires_gpu,
    };
    append_user_model(data_dir, entry.clone())?;
    Ok(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rejects_unknown_runtime() {
        let err = parse_engine("whisper").unwrap_err();
        assert!(matches!(err, ImportError::InvalidEngine(_)));
    }

    #[test]
    fn llm_requires_single_non_mmproj_gguf() {
        let dir = tempfile::tempdir().unwrap();
        let gguf = dir.path().join("model.gguf");
        std::fs::write(&gguf, b"x").unwrap();
        validate_files(ModelPurpose::Llm, ModelEngine::Llama, &[gguf]).unwrap();
    }

    #[test]
    fn llama_asr_requires_mmproj() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("model.gguf");
        std::fs::write(&main, b"x").unwrap();
        let err = validate_files(ModelPurpose::Stt, ModelEngine::Llama, &[main]).unwrap_err();
        assert!(matches!(err, ImportError::MissingLlamaAsrFiles));
    }

    #[test]
    fn import_writes_user_manifest_and_managed_files() {
        let data = tempfile::tempdir().unwrap();
        let source = tempfile::tempdir().unwrap();
        let gguf = source.path().join("model.gguf");
        std::fs::write(&gguf, b"hello").unwrap();

        let entry = import_model(
            data.path(),
            ImportLocalModelRequest {
                display_name: "Demo Model".into(),
                purpose: "llm".into(),
                engine: "llama".into(),
                files: vec![gguf.display().to_string()],
                license: "MIT".into(),
                min_ram_gb: 4,
                requires_gpu: false,
            },
        )
        .unwrap();

        assert!(entry.id.starts_with("user-demo-model-"));
        assert!(
            manifest::model_dir(data.path(), &entry.id)
                .join("model.gguf")
                .exists()
        );
        let imported = manifest::load_user_catalog(data.path());
        assert_eq!(imported, vec![entry]);
    }
}
