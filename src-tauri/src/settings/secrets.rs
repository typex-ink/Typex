//! keyring 封装：`keyring://typex/<slot>/<profile-id>/<field>` 引用解析（07 §9）。
//! settings.json 只存引用，明文只在 OS 凭据库。

use crate::error::{ErrorCode, Result, TypexError};

const SERVICE: &str = "typex";
const PREFIX: &str = "keyring://typex/";

/// 由槽位/档案/字段构造 keyring 引用字符串（写入 settings.json 的形态）。
pub fn make_ref(slot: &str, profile_id: &str, field: &str) -> String {
    format!("{PREFIX}{slot}/{profile_id}/{field}")
}

fn account_of(reference: &str) -> Result<&str> {
    reference
        .strip_prefix(PREFIX)
        .ok_or_else(|| TypexError::new(ErrorCode::Internal, format!("非法密钥引用: {reference}")))
}

/// 密钥存取抽象——测试用内存实现替换（08 §4.2）。
pub trait SecretStore: Send + Sync {
    fn set(&self, reference: &str, secret: &str) -> Result<()>;
    fn get(&self, reference: &str) -> Result<String>;
    fn delete(&self, reference: &str) -> Result<()>;
}

/// OS 凭据库实现（macOS Keychain / Windows Credential Manager / Secret Service）。
pub struct KeyringStore;

impl SecretStore for KeyringStore {
    fn set(&self, reference: &str, secret: &str) -> Result<()> {
        let account = account_of(reference)?;
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("写入凭据库失败: {e}")))
    }

    fn get(&self, reference: &str) -> Result<String> {
        let account = account_of(reference)?;
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.get_password())
            .map_err(|e| TypexError::new(ErrorCode::AuthError, format!("读取凭据失败: {e}")))
    }

    fn delete(&self, reference: &str) -> Result<()> {
        let account = account_of(reference)?;
        keyring::Entry::new(SERVICE, account)
            .and_then(|e| e.delete_credential())
            .map_err(|e| TypexError::new(ErrorCode::Internal, format!("删除凭据失败: {e}")))
    }
}

/// 测试/无凭据库环境用的内存实现。
#[derive(Default)]
pub struct MemoryStore(std::sync::Mutex<std::collections::HashMap<String, String>>);

impl SecretStore for MemoryStore {
    fn set(&self, reference: &str, secret: &str) -> Result<()> {
        account_of(reference)?;
        self.0.lock().unwrap().insert(reference.to_string(), secret.to_string());
        Ok(())
    }

    fn get(&self, reference: &str) -> Result<String> {
        account_of(reference)?;
        self.0
            .lock()
            .unwrap()
            .get(reference)
            .cloned()
            .ok_or_else(|| TypexError::new(ErrorCode::AuthError, "凭据不存在"))
    }

    fn delete(&self, reference: &str) -> Result<()> {
        self.0.lock().unwrap().remove(reference);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_roundtrip_via_memory_store() {
        let store = MemoryStore::default();
        let r = make_ref("stt", "groq-fast", "api_key");
        assert_eq!(r, "keyring://typex/stt/groq-fast/api_key");
        store.set(&r, "sk-123").unwrap();
        assert_eq!(store.get(&r).unwrap(), "sk-123");
        store.delete(&r).unwrap();
        assert!(store.get(&r).is_err());
    }

    #[test]
    fn invalid_ref_rejected() {
        let store = MemoryStore::default();
        assert!(store.get("sk-plaintext-key").is_err());
    }
}
