//! Windows DPAPI protection for API keys at rest (Phase 3 / E1).
//!
//! On-disk form: `vfdpapi1:` + base64(CryptProtectData bytes).
//! Plaintext legacy values are accepted on load and re-encrypted on next save.
//! Keys are never logged.

use base64::Engine as _;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

const PREFIX: &str = "vfdpapi1:";

/// Protect a secret for the current Windows user (DPAPI). Empty stays empty.
pub fn protect_secret(plain: &str) -> anyhow::Result<String> {
    let plain = plain.trim();
    if plain.is_empty() {
        return Ok(String::new());
    }
    // Already protected (idempotent save).
    if plain.starts_with(PREFIX) {
        return Ok(plain.to_string());
    }

    let mut bytes = plain.as_bytes().to_vec();
    let mut blob_in = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_mut_ptr(),
    };
    let mut blob_out = CRYPT_INTEGER_BLOB::default();

    // SAFETY: CryptProtectData allocates blob_out; free with LocalFree.
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
        Ok(format!("{PREFIX}{encoded}"))
    }
}

/// Unprotect a secret. Plaintext (no prefix) is returned unchanged for migration.
pub fn unprotect_secret(stored: &str) -> anyhow::Result<String> {
    let stored = stored.trim();
    if stored.is_empty() {
        return Ok(String::new());
    }
    if !stored.starts_with(PREFIX) {
        return Ok(stored.to_string());
    }
    let b64 = &stored[PREFIX.len()..];
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

/// Decrypt secrets in settings after JSON load (in-memory plaintext for the app).
pub fn unprotect_settings_secrets(settings: &mut vf_core::Settings) {
    for key in &mut settings.stt.api_keys {
        match unprotect_secret(key) {
            Ok(p) => *key = p,
            Err(e) => {
                log::error!("failed to unprotect ElevenLabs key: {e}");
            }
        }
    }
    match unprotect_secret(&settings.llm.api_key) {
        Ok(p) => settings.llm.api_key = p,
        Err(e) => log::error!("failed to unprotect Groq key: {e}"),
    }
}

/// Encrypt secrets for disk write. Operates on a clone so live settings stay plaintext.
pub fn protect_settings_for_disk(settings: &vf_core::Settings) -> anyhow::Result<vf_core::Settings> {
    let mut out = settings.clone();
    let mut protected = Vec::with_capacity(out.stt.api_keys.len());
    for key in &out.stt.api_keys {
        protected.push(protect_secret(key)?);
    }
    out.stt.api_keys = protected;
    out.llm.api_key = protect_secret(&out.llm.api_key)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_protect() {
        let plain = "sk_test_secret_value_123";
        let sealed = protect_secret(plain).expect("protect");
        assert!(sealed.starts_with(PREFIX));
        assert_ne!(sealed, plain);
        let open = unprotect_secret(&sealed).expect("unprotect");
        assert_eq!(open, plain);
    }

    #[test]
    fn plaintext_legacy_passthrough() {
        let plain = "gsk_legacy_plaintext";
        assert_eq!(unprotect_secret(plain).unwrap(), plain);
    }

    #[test]
    fn empty_ok() {
        assert_eq!(protect_secret("").unwrap(), "");
        assert_eq!(unprotect_secret("").unwrap(), "");
    }
}
