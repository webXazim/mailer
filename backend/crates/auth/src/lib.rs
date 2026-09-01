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

pub fn token_matches(left: &str, right: &str) -> bool {
    hash_token(left)
        .iter()
        .zip(hash_token(right))
        .fold(0u8, |diff, (a, b)| diff | (a ^ b))
        == 0
}

/// Webhooks require a DNS hostname; literal addresses bypass Hyper's DNS resolver.
pub fn public_webhook_url(value: &str) -> Result<url::Url> {
    let url = url::Url::parse(value)?;
    anyhow::ensure!(
        value.len() <= 2048
            && url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "Use an HTTPS URL without credentials or fragments"
    );
    match url.host() {
        Some(url::Host::Domain(host))
            if host.contains('.')
                && !host.ends_with('.')
                && host != "localhost"
                && !host.ends_with(".localhost") =>
        {
            Ok(url)
        }
        _ => anyhow::bail!("Webhook URLs require a public DNS hostname, not an IP address"),
    }
}

pub fn is_public_ip(ip: std::net::IpAddr) -> bool {
    use std::net::IpAddr;
    match ip {
        IpAddr::V4(v) => {
            let [a, b, _, _] = v.octets();
            !(v.is_private()
                || v.is_loopback()
                || v.is_link_local()
                || v.is_unspecified()
                || v.is_broadcast()
                || v.is_documentation()
                || a == 0
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 198 && (b == 18 || b == 19))
                || (a == 192 && b == 0))
        }
        IpAddr::V6(v) => match v.to_ipv4_mapped() {
            Some(v4) => is_public_ip(IpAddr::V4(v4)),
            None => {
                (v.segments()[0] & 0xe000) == 0x2000
                    && !(v.segments()[0] == 0x2001 && v.segments()[1] < 0x200)
                    && !(v.segments()[0] == 0x2001 && v.segments()[1] == 0xdb8)
                    && v.segments()[0] != 0x2002
            }
        },
    }
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
    fn webhook_destinations_reject_private_and_literal_addresses() {
        for value in [
            "https://[::1]/",
            "https://[::ffff:127.0.0.1]/",
            "https://127.0.0.1/",
            "http://hooks.example.com",
            "https://localhost/",
            "https://hooks.localhost/",
        ] {
            assert!(public_webhook_url(value).is_err(), "{value}");
        }
        assert!(public_webhook_url("https://hooks.example.com/events").is_ok());
        for value in [
            "127.0.0.1",
            "10.0.0.1",
            "169.254.169.254",
            "100.64.0.1",
            "::1",
            "::ffff:10.1.1.1",
            "fc00::1",
            "fe80::1",
        ] {
            assert!(!is_public_ip(value.parse().unwrap()), "{value}");
        }
        assert!(is_public_ip("8.8.8.8".parse().unwrap()));
    }

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
