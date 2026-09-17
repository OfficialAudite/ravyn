use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::{Rng, RngCore};
use sha2::{Digest, Sha256};
use thiserror::Error;
use totp_rs::{Algorithm, Builder, Secret, Totp};

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("failed to hash password")]
    Hash,
}

pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::Hash)
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    let Ok(parsed_hash) = PasswordHash::new(hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

/// Generates a random, high-entropy opaque token (for session cookies and API
/// tokens). The returned value is shown to the caller once; only its hash is
/// ever stored.
pub fn generate_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// A fresh, base32-encoded TOTP secret — stored as-is (not hashed, unlike a
/// password) since verifying a code requires computing the current code
/// from it, not just comparing a digest.
pub fn generate_totp_secret() -> String {
    Secret::generate().to_base32()
}

/// Builds the `Totp` object shared by both QR/manual-entry setup and code
/// verification, so the two can never disagree about algorithm, digits, or
/// step — every self-hosted instance's TOTP config lives in this one place.
/// `None` only if `secret` isn't valid base32, which never happens for a
/// secret this crate generated itself.
fn build_totp(secret: &str, account_name: &str, issuer: &str) -> Option<Totp> {
    let secret = Secret::try_from_base32(secret).ok()?;
    Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(6)
        .with_skew(1)
        .with_step_duration(30)
        .with_secret(secret)
        .with_issuer(Some(issuer.to_string()))
        .with_account_name(account_name.to_string())
        .build()
        .ok()
}

/// The `otpauth://` URI an authenticator app scans (as a QR code) or accepts
/// via manual entry — `(url, qr_code_base64_png)`. `None` if `secret` is
/// somehow invalid.
pub fn totp_setup_uri(secret: &str, account_name: &str, issuer: &str) -> Option<(String, String)> {
    let totp = build_totp(secret, account_name, issuer)?;
    let url = totp.to_url().ok()?;
    let qr = totp.to_qr_base64().ok()?;
    Some((url, qr))
}

pub fn verify_totp(secret: &str, code: &str) -> bool {
    let Some(totp) = build_totp(secret, "", "") else {
        return false;
    };
    totp.check_current(code).is_some()
}

/// Single-use backup codes, generated once when 2FA is first enabled and
/// shown to the user exactly then — same "shown once" treatment as an API
/// token or invite code. Formatted in two groups (`xxxx-xxxx`) purely for
/// readability; the entropy is what matters, not the shape.
pub fn generate_recovery_codes(count: usize) -> Vec<String> {
    const CHARSET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..count)
        .map(|_| {
            let group = |rng: &mut rand::rngs::ThreadRng| -> String {
                (0..4)
                    .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
                    .collect()
            };
            format!("{}-{}", group(&mut rng), group(&mut rng))
        })
        .collect()
}

/// A random slug for a shortened URL — lowercase alphanumeric, no
/// ambiguous characters, so it reads back cleanly if someone has to type
/// it. Collisions are handled by the caller retrying with a fresh one
/// against the database's own uniqueness constraint, not by checking here.
pub fn generate_slug(length: usize) -> String {
    const CHARSET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}
