use std::{env, fs, process};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use minisign_verify::{PublicKey, Signature};

fn required_env(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| format!("{name} is required"))
}

fn decode_tauri_value(value: &str, label: &str) -> Result<String, String> {
    let decoded = STANDARD
        .decode(value.trim())
        .map_err(|error| format!("invalid base64 {label}: {error}"))?;
    String::from_utf8(decoded).map_err(|error| format!("invalid UTF-8 {label}: {error}"))
}

fn verify() -> Result<(), String> {
    let artifact_path = required_env("TYPEX_UPDATER_ARTIFACT")?;
    let signature_path = required_env("TYPEX_UPDATER_SIGNATURE")?;
    let public_key =
        decode_tauri_value(&required_env("TAURI_UPDATER_PUBKEY")?, "updater public key")?;
    let signature = decode_tauri_value(
        &fs::read_to_string(&signature_path)
            .map_err(|error| format!("failed to read updater signature: {error}"))?,
        "updater signature",
    )?;
    let artifact = fs::read(&artifact_path)
        .map_err(|error| format!("failed to read updater artifact: {error}"))?;

    let public_key = PublicKey::decode(&public_key)
        .map_err(|error| format!("invalid updater public key: {error}"))?;
    let signature = Signature::decode(&signature)
        .map_err(|error| format!("invalid updater signature: {error}"))?;
    public_key
        .verify(&artifact, &signature, true)
        .map_err(|error| format!("updater signature verification failed: {error}"))
}

fn main() {
    if let Err(error) = verify() {
        eprintln!("{error}");
        process::exit(1);
    }
}
