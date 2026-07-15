//! Secret protection for API keys at rest.
//!
//! - **DPAPI** (`vfdpapi1:`): bound to the current Windows user (default).
//! - **Passphrase vault**: AES-256-GCM sealed blob (portable across machines).
//!
//! Keys are never logged. In-memory settings always hold plaintext after unlock.

use std::sync::Mutex;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine as _;
use pbkdf2::pbkdf2_hmac;
use rand::RngCore;
use sha2::Sha256;
use vf_core::{Settings, VaultMode, VaultSealed};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};
use zeroize::Zeroize;

const DPAPI_PREFIX: &str = "vfdpapi1:";
const PBKDF2_ITERS: u32 = 210_000;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 12;
const KEY_LEN: usize = 32;

static SESSION_KEY: Mutex<Option<[u8; KEY_LEN]>> = Mutex::new(None);
static SESSION_PASS: Mutex<Option<String>> = Mutex::new(None);

#[derive(serde::Serialize, serde::Deserialize)]
struct SecretsPayload {
    api_keys: Vec<String>,
    llm_key: String,
}

// --- DPAPI ---

pub fn protect_secret_dpapi(plain: &str) -> anyhow::Result<String> {
    let plain = plain.trim();
    if plain.is_empty() {
        return Ok(String::new());
    }
    if plain.starts_with(DPAPI_PREFIX) {
        return Ok(plain.to_string());
    }

    let mut bytes = plain.as_bytes().to_vec();
    let mut blob_in = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut blob_out = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptProtectData(
            &mut blob_in,
            PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut blob_out,
        )
        .map_err(|e| anyhow::anyhow!("DPAPI CryptProtectData failed: {e}"))?;

        if blob_out.pbData.is_null() || blob_out.cbData == 0 {
            return Err(anyhow::anyhow!("DPAPI returned empty blob"));
        }
        let slice = std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize);
        let encoded = base64::engine::general_purpose::STANDARD.encode(slice);
        let _ = LocalFree(Some(HLOCAL(blob_out.pbData as *mut _)));
        Ok(format!("{DPAPI_PREFIX}{encoded}"))
    }
}

pub fn unprotect_secret_dpapi(stored: &str) -> anyhow::Result<String> {
    let stored = stored.trim();
    if stored.is_empty() {
        return Ok(String::new());
    }
    if !stored.starts_with(DPAPI_PREFIX) {
        return Ok(stored.to_string());
    }
    let b64 = &stored[DPAPI_PREFIX.len()..];
    let mut encrypted = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| anyhow::anyhow!("DPAPI blob base64 decode failed: {e}"))?;

    let mut blob_in = CRYPT_INTEGER_BLOB {
        cbData: encrypted.len() as u32,
        pbData: encrypted.as_mut_ptr(),
    };
    let mut blob_out = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptUnprotectData(
            &mut blob_in,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut blob_out,
        )
        .map_err(|e| anyhow::anyhow!("DPAPI CryptUnprotectData failed: {e}"))?;

        if blob_out.pbData.is_null() || blob_out.cbData == 0 {
            return Err(anyhow::anyhow!("DPAPI unprotect returned empty"));
        }
        let slice = std::slice::from_raw_parts(blob_out.pbData, blob_out.cbData as usize);
        let plain = String::from_utf8(slice.to_vec())
            .map_err(|e| anyhow::anyhow!("DPAPI plaintext not UTF-8: {e}"))?;
        let _ = LocalFree(Some(HLOCAL(blob_out.pbData as *mut _)));
        Ok(plain)
    }
}

pub fn protect_secret(plain: &str) -> anyhow::Result<String> {
    protect_secret_dpapi(plain)
}

pub fn unprotect_secret(stored: &str) -> anyhow::Result<String> {
    unprotect_secret_dpapi(stored)
}

// --- Passphrase AES-GCM ---

fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    pbkdf2_hmac::<Sha256>(passphrase.as_bytes(), salt, PBKDF2_ITERS, &mut key);
    key
}

fn seal_payload(passphrase: &str, payload: &SecretsPayload) -> anyhow::Result<VaultSealed> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let mut key = derive_key(passphrase, &salt);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("AES key init: {e}"))?;
    key.zeroize();

    let plain = serde_json::to_vec(payload)?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plain.as_ref())
        .map_err(|_| anyhow::anyhow!("AES-GCM encrypt failed"))?;

    Ok(VaultSealed {
        salt_b64: base64::engine::general_purpose::STANDARD.encode(salt),
        nonce_b64: base64::engine::general_purpose::STANDARD.encode(nonce_bytes),
        ciphertext_b64: base64::engine::general_purpose::STANDARD.encode(ciphertext),
    })
}

fn open_payload(
    passphrase: &str,
    sealed: &VaultSealed,
) -> anyhow::Result<(SecretsPayload, [u8; KEY_LEN])> {
    let salt = base64::engine::general_purpose::STANDARD
        .decode(&sealed.salt_b64)
        .map_err(|e| anyhow::anyhow!("salt decode: {e}"))?;
    let nonce_bytes = base64::engine::general_purpose::STANDARD
        .decode(&sealed.nonce_b64)
        .map_err(|e| anyhow::anyhow!("nonce decode: {e}"))?;
    let ciphertext = base64::engine::general_purpose::STANDARD
        .decode(&sealed.ciphertext_b64)
        .map_err(|e| anyhow::anyhow!("ciphertext decode: {e}"))?;

    if nonce_bytes.len() != NONCE_LEN {
        anyhow::bail!("invalid nonce length");
    }

    let key = derive_key(passphrase, &salt);
    let cipher =
        Aes256Gcm::new_from_slice(&key).map_err(|e| anyhow::anyhow!("AES key init: {e}"))?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let plain = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| anyhow::anyhow!("Wrong passphrase or corrupted vault"))?;
    let payload: SecretsPayload = serde_json::from_slice(&plain)?;
    Ok((payload, key))
}

pub fn vault_session_active() -> bool {
    SESSION_KEY.lock().map(|g| g.is_some()).unwrap_or(false)
}

pub fn vault_clear_session() {
    if let Ok(mut g) = SESSION_KEY.lock() {
        if let Some(ref mut k) = *g {
            k.zeroize();
        }
        *g = None;
    }
    if let Ok(mut g) = SESSION_PASS.lock() {
        if let Some(ref mut p) = *g {
            p.zeroize();
        }
        *g = None;
    }
}

pub fn vault_needs_unlock(settings: &Settings) -> bool {
    settings.vault.mode == VaultMode::Passphrase
        && settings.vault.sealed.is_some()
        && !vault_session_active()
}

/// Unlock passphrase vault into `settings` (fills api keys).
pub fn vault_unlock(settings: &mut Settings, passphrase: &str) -> anyhow::Result<()> {
    if settings.vault.mode != VaultMode::Passphrase {
        anyhow::bail!("Vault is not in passphrase mode");
    }
    let sealed = settings
        .vault
        .sealed
        .clone()
        .ok_or_else(|| anyhow::anyhow!("No sealed vault — enable passphrase mode first"))?;
    let (payload, key) = open_payload(passphrase, &sealed)?;
    settings.stt.api_keys = payload.api_keys;
    settings.llm.api_key = payload.llm_key;
    *SESSION_KEY.lock().map_err(|e| anyhow::anyhow!("{e}"))? = Some(key);
    *SESSION_PASS.lock().map_err(|e| anyhow::anyhow!("{e}"))? = Some(passphrase.to_string());
    Ok(())
}

/// Switch to passphrase mode and seal current in-memory keys.
pub fn vault_enable_passphrase(settings: &mut Settings, passphrase: &str) -> anyhow::Result<()> {
    if passphrase.chars().count() < 8 {
        anyhow::bail!("Passphrase must be at least 8 characters");
    }
    let payload = SecretsPayload {
        api_keys: settings.stt.api_keys.clone(),
        llm_key: settings.llm.api_key.clone(),
    };
    let sealed = seal_payload(passphrase, &payload)?;
    let (_, key) = open_payload(passphrase, &sealed)?;
    settings.vault.mode = VaultMode::Passphrase;
    settings.vault.sealed = Some(sealed);
    *SESSION_KEY.lock().map_err(|e| anyhow::anyhow!("{e}"))? = Some(key);
    *SESSION_PASS.lock().map_err(|e| anyhow::anyhow!("{e}"))? = Some(passphrase.to_string());
    Ok(())
}

/// Switch back to DPAPI (keys must already be in memory).
pub fn vault_enable_dpapi(settings: &mut Settings) -> anyhow::Result<()> {
    settings.vault.mode = VaultMode::Dpapi;
    settings.vault.sealed = None;
    vault_clear_session();
    Ok(())
}

pub fn unprotect_settings_secrets(settings: &mut Settings) {
    match settings.vault.mode {
        VaultMode::Dpapi => {
            for key in &mut settings.stt.api_keys {
                match unprotect_secret_dpapi(key) {
                    Ok(p) => *key = p,
                    Err(e) => log::error!("failed to unprotect ElevenLabs key: {e}"),
                }
            }
            match unprotect_secret_dpapi(&settings.llm.api_key) {
                Ok(p) => settings.llm.api_key = p,
                Err(e) => log::error!("failed to unprotect Groq key: {e}"),
            }
            settings.vault.sealed = None;
        }
        VaultMode::Passphrase => {
            // Sealed blob holds secrets; key fields stay empty until unlock.
            // If plaintext keys remain (migration), keep them until next save.
        }
    }
}

pub fn protect_settings_for_disk(settings: &Settings) -> anyhow::Result<Settings> {
    let mut out = settings.clone();
    match out.vault.mode {
        VaultMode::Dpapi => {
            out.vault.sealed = None;
            let mut protected = Vec::with_capacity(out.stt.api_keys.len());
            for key in &out.stt.api_keys {
                protected.push(protect_secret_dpapi(key)?);
            }
            out.stt.api_keys = protected;
            out.llm.api_key = protect_secret_dpapi(&out.llm.api_key)?;
        }
        VaultMode::Passphrase => {
            let pass = SESSION_PASS
                .lock()
                .map_err(|e| anyhow::anyhow!("{e}"))?
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Unlock the vault before saving keys"))?;
            let payload = SecretsPayload {
                api_keys: settings.stt.api_keys.clone(),
                llm_key: settings.llm.api_key.clone(),
            };
            out.vault.sealed = Some(seal_payload(&pass, &payload)?);
            out.stt.api_keys.clear();
            out.llm.api_key.clear();
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_dpapi() {
        let plain = "sk_test_secret_value_123";
        let sealed = protect_secret_dpapi(plain).expect("protect");
        assert!(sealed.starts_with(DPAPI_PREFIX));
        assert_eq!(unprotect_secret_dpapi(&sealed).unwrap(), plain);
    }

    #[test]
    fn passphrase_round_trip() {
        let mut s = Settings::default();
        s.stt.api_keys = vec!["el_key_1".into()];
        s.llm.api_key = "gsk_test".into();
        vault_enable_passphrase(&mut s, "correct horse battery").unwrap();
        assert_eq!(s.vault.mode, VaultMode::Passphrase);

        let disk = protect_settings_for_disk(&s).unwrap();
        assert!(disk.stt.api_keys.is_empty());
        assert!(disk.llm.api_key.is_empty());
        assert!(disk.vault.sealed.is_some());

        vault_clear_session();
        let mut locked = disk;
        unprotect_settings_secrets(&mut locked);
        assert!(vault_needs_unlock(&locked));

        vault_unlock(&mut locked, "correct horse battery").unwrap();
        assert_eq!(locked.stt.api_keys, vec!["el_key_1".to_string()]);
        assert_eq!(locked.llm.api_key, "gsk_test");
        vault_clear_session();
    }

    #[test]
    fn wrong_passphrase_fails() {
        let mut s = Settings::default();
        s.llm.api_key = "secret".into();
        vault_enable_passphrase(&mut s, "good-passphrase").unwrap();
        let disk = protect_settings_for_disk(&s).unwrap();
        vault_clear_session();
        let mut locked = disk;
        assert!(vault_unlock(&mut locked, "bad-passphrase").is_err());
    }

    #[test]
    fn empty_ok() {
        assert_eq!(protect_secret_dpapi("").unwrap(), "");
        assert_eq!(unprotect_secret_dpapi("").unwrap(), "");
    }
}
