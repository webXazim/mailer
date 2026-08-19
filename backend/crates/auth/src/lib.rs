use anyhow::Result;
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

pub fn verify_password(password: &str, encoded_hash: &str) -> bool {
    PasswordHash::new(encoded_hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

pub fn generate_token() -> String {
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

pub fn webhook_secret(master_key: &str, endpoint_id: Uuid, version: i32) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(master_key.as_bytes())
        .expect("HMAC accepts keys of any length");
    mac.update(endpoint_id.as_bytes());
    mac.update(&version.to_be_bytes());
    format!(
        "whsec_{}",
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    )
}

pub fn webhook_signature(secret: &str, webhook_id: &str, timestamp: i64, body: &[u8]) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(webhook_id.as_bytes());
    mac.update(b".");
    mac.update(timestamp.to_string().as_bytes());
    mac.update(b".");
    mac.update(body);
    format!("v1,{}", URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_hash_and_verify() {
        let hash = hash_password("a-long-development-password").expect("hash password");
        assert!(verify_password("a-long-development-password", &hash));
        assert!(!verify_password("wrong-password", &hash));
    }

    #[test]
    fn tokens_are_random_and_hashed() {
        let first = generate_token();
        let second = generate_token();
        assert_ne!(first, second);
        assert_ne!(hash_token(&first), hash_token(&second));
        assert_eq!(hash_token(&first).len(), 32);
    }

    #[test]
    fn webhook_secrets_and_signatures_are_deterministic() {
        let endpoint_id = Uuid::nil();
        let first = webhook_secret("master-key", endpoint_id, 1);
        let rotated = webhook_secret("master-key", endpoint_id, 2);
        assert!(first.starts_with("whsec_"));
        assert_ne!(first, rotated);
        assert_eq!(
            webhook_signature(&first, "delivery-id", 123, b"{}"),
            webhook_signature(&first, "delivery-id", 123, b"{}")
        );
    }
}
