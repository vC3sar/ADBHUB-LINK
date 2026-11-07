//! # `adbhub-auth`
//!
//! Implements the **Users** entity (Section 8) and authentication primitives (Section 9 & 12):
//! - Salted iterative SHA-256 / HMAC-SHA256 password hashing (no plaintext passwords ever stored).
//! - Signed Access Tokens (`AccessToken`) and rotating Refresh Tokens (`RefreshToken`) with expiration.
//! - Role and status verification (`Active`, `Suspended`, `Disabled`).

use std::collections::HashMap;

use adbhub_core::{AdbHubError, CoreResult};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

/// Account status for a user (`Users.status` in Section 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UserStatus {
    Active,
    Suspended,
    Disabled,
}

/// Domain model for `Users` (Section 8).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub created_at: DateTime<Utc>,
    pub status: UserStatus,
}

/// Public user profile returned by REST endpoints (never exposes `password_hash`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicUserProfile {
    pub id: Uuid,
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub status: UserStatus,
}

impl From<&User> for PublicUserProfile {
    fn from(u: &User) -> Self {
        Self {
            id: u.id,
            email: u.email.clone(),
            created_at: u.created_at,
            status: u.status,
        }
    }
}

/// Claims encoded inside an API Access or Refresh token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthTokenClaims {
    pub sub: Uuid,
    pub email: String,
    pub token_type: AuthTokenKind,
    pub jti: Uuid,
    pub iat: i64,
    pub exp: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthTokenKind {
    Access,
    Refresh,
}

/// Pair of signed tokens returned on login/register/refresh.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenPairResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in_seconds: i64,
    pub user: PublicUserProfile,
}

/// Derives a salted iterative PBKDF-like SHA-256 password hash (`v1$<salt_hex>$<hash_hex>`).
pub fn hash_password(password: &str) -> String {
    let salt = Uuid::new_v4();
    let salt_hex = hex::encode(salt.as_bytes());
    let digest = compute_salted_rounds(password, &salt_hex, 10_000);
    format!("v1${salt_hex}${digest}")
}

/// Verifies a plaintext password against a stored `v1$<salt_hex>$<hash_hex>` string.
pub fn verify_password(password: &str, stored_hash: &str) -> bool {
    let parts: Vec<&str> = stored_hash.split('$').collect();
    if parts.len() != 3 || parts[0] != "v1" {
        return false;
    }
    let salt_hex = parts[1];
    let expected = parts[2];
    let computed = compute_salted_rounds(password, salt_hex, 10_000);
    constant_time_eq(computed.as_bytes(), expected.as_bytes())
}

fn compute_salted_rounds(password: &str, salt_hex: &str, rounds: u32) -> String {
    let mut current = {
        let mut h = Sha256::new();
        h.update(b"ADBHUB-PWD-V1:");
        h.update(salt_hex.as_bytes());
        h.update(b":");
        h.update(password.as_bytes());
        h.finalize()
    };

    for r in 0..rounds {
        let mut h = Sha256::new();
        h.update(current);
        h.update(r.to_le_bytes());
        h.update(password.as_bytes());
        current = h.finalize();
    }

    hex::encode(current)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Encodes and signs [`AuthTokenClaims`] using HMAC-SHA256 (`<claims_hex>.<sig_hex>`).
pub fn encode_signed_token(claims: &AuthTokenClaims, secret: &[u8]) -> CoreResult<String> {
    let json_bytes = serde_json::to_vec(claims)
        .map_err(|e| AdbHubError::WindowsUsbError(format!("Token claims serialize error: {e}")))?;
    let payload_hex = hex::encode(&json_bytes);

    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|e| AdbHubError::WindowsUsbError(format!("HMAC key error: {e}")))?;
    mac.update(payload_hex.as_bytes());
    let sig_hex = hex::encode(mac.finalize().into_bytes());

    Ok(format!("{payload_hex}.{sig_hex}"))
}

/// Decodes and verifies a signed token string (`<claims_hex>.<sig_hex>`).
pub fn decode_and_verify_token(
    token_str: &str,
    secret: &[u8],
    expected_kind: AuthTokenKind,
) -> CoreResult<AuthTokenClaims> {
    let (payload_hex, sig_hex) = token_str
        .trim()
        .split_once('.')
        .ok_or_else(|| AdbHubError::WindowsUsbError("Malformed auth token format".to_string()))?;

    let sig_bytes = hex::decode(sig_hex).map_err(|_| {
        AdbHubError::WindowsUsbError("Malformed auth token signature hex".to_string())
    })?;

    let mut mac = HmacSha256::new_from_slice(secret)
        .map_err(|e| AdbHubError::WindowsUsbError(format!("HMAC key error: {e}")))?;
    mac.update(payload_hex.as_bytes());
    mac.verify_slice(&sig_bytes).map_err(|_| {
        AdbHubError::WindowsUsbError("Invalid auth token cryptographic signature".to_string())
    })?;

    let json_bytes = hex::decode(payload_hex).map_err(|_| {
        AdbHubError::WindowsUsbError("Malformed auth token payload hex".to_string())
    })?;
    let claims: AuthTokenClaims = serde_json::from_slice(&json_bytes).map_err(|e| {
        AdbHubError::WindowsUsbError(format!("Malformed auth token claims JSON: {e}"))
    })?;

    if claims.token_type != expected_kind {
        return Err(AdbHubError::WindowsUsbError(format!(
            "Expected {:?} token, got {:?}",
            expected_kind, claims.token_type
        )));
    }

    let now = Utc::now().timestamp();
    if now > claims.exp {
        return Err(AdbHubError::WindowsUsbError(
            "Authentication token has expired".to_string(),
        ));
    }

    Ok(claims)
}

/// Authentication & User Management Service with optional local JSON file persistence (zero external DB servers).
pub struct AuthService {
    jwt_secret: Vec<u8>,
    storage_path: Option<std::path::PathBuf>,
    users_by_id: RwLock<HashMap<Uuid, User>>,
    users_by_email: RwLock<HashMap<String, Uuid>>,
}

impl AuthService {
    /// Creates an in-memory [`AuthService`].
    pub fn new(jwt_secret: Vec<u8>) -> Self {
        Self {
            jwt_secret,
            storage_path: None,
            users_by_id: RwLock::new(HashMap::new()),
            users_by_email: RwLock::new(HashMap::new()),
        }
    }

    /// Creates an [`AuthService`] backed by a local file (`users.json`), loading any existing accounts on startup.
    pub fn with_file_storage(jwt_secret: Vec<u8>, path: impl Into<std::path::PathBuf>) -> CoreResult<Self> {
        let storage_path = path.into();
        let mut by_id = HashMap::new();
        let mut by_email = HashMap::new();

        if storage_path.exists() {
            let bytes = std::fs::read(&storage_path).map_err(|e| {
                AdbHubError::WindowsUsbError(format!(
                    "Failed reading local users file '{}': {e}",
                    storage_path.display()
                ))
            })?;
            if !bytes.is_empty() {
                let loaded: Vec<User> = serde_json::from_slice(&bytes).map_err(|e| {
                    AdbHubError::WindowsUsbError(format!(
                        "Corrupted local users file '{}': {e}",
                        storage_path.display()
                    ))
                })?;
                for u in loaded {
                    by_email.insert(u.email.clone(), u.id);
                    by_id.insert(u.id, u);
                }
            }
        }

        Ok(Self {
            jwt_secret,
            storage_path: Some(storage_path),
            users_by_id: RwLock::new(by_id),
            users_by_email: RwLock::new(by_email),
        })
    }

    fn persist_users_snapshot(&self, users: &HashMap<Uuid, User>) -> CoreResult<()> {
        if let Some(path) = &self.storage_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    AdbHubError::WindowsUsbError(format!(
                        "Failed creating data directory '{}': {e}",
                        parent.display()
                    ))
                })?;
            }
            let list: Vec<&User> = users.values().collect();
            let serialized = serde_json::to_vec_pretty(&list).map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed serializing users: {e}"))
            })?;
            let tmp_path = path.with_extension("json.tmp");
            std::fs::write(&tmp_path, serialized).map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed writing temp users file: {e}"))
            })?;
            std::fs::rename(&tmp_path, path).map_err(|e| {
                AdbHubError::WindowsUsbError(format!("Failed committing users file: {e}"))
            })?;
        }
        Ok(())
    }

    /// Registers a new user account (`POST /api/v1/auth/register`).
    pub async fn register_user(&self, email: &str, password: &str) -> CoreResult<TokenPairResponse> {
        let clean_email = email.trim().to_ascii_lowercase();
        if !clean_email.contains('@') || clean_email.len() < 5 {
            return Err(AdbHubError::WindowsUsbError(
                "Invalid email address".to_string(),
            ));
        }
        if password.len() < 8 {
            return Err(AdbHubError::WindowsUsbError(
                "Password must be at least 8 characters long".to_string(),
            ));
        }

        let mut email_map = self.users_by_email.write().await;
        if email_map.contains_key(&clean_email) {
            return Err(AdbHubError::WindowsUsbError(format!(
                "Email '{clean_email}' is already registered"
            )));
        }

        let user = User {
            id: Uuid::new_v4(),
            email: clean_email.clone(),
            password_hash: hash_password(password),
            created_at: Utc::now(),
            status: UserStatus::Active,
        };

        let profile = PublicUserProfile::from(&user);
        email_map.insert(clean_email, user.id);
        let mut id_map = self.users_by_id.write().await;
        id_map.insert(user.id, user);
        self.persist_users_snapshot(&id_map)?;
        drop(id_map);
        drop(email_map);

        self.issue_tokens_for_profile(profile)
    }

    /// Authenticates an existing user (`POST /api/v1/auth/login`).
    pub async fn login_user(&self, email: &str, password: &str) -> CoreResult<TokenPairResponse> {
        let clean_email = email.trim().to_ascii_lowercase();
        let user_id = self
            .users_by_email
            .read()
            .await
            .get(&clean_email)
            .copied()
            .ok_or_else(|| {
                AdbHubError::WindowsUsbError("Invalid email or password".to_string())
            })?;

        let users = self.users_by_id.read().await;
        let user = users.get(&user_id).ok_or_else(|| {
            AdbHubError::WindowsUsbError("Invalid email or password".to_string())
        })?;

        if user.status != UserStatus::Active {
            return Err(AdbHubError::WindowsUsbError(
                "User account is not active".to_string(),
            ));
        }

        if !verify_password(password, &user.password_hash) {
            return Err(AdbHubError::WindowsUsbError(
                "Invalid email or password".to_string(),
            ));
        }

        let profile = PublicUserProfile::from(user);
        drop(users);

        self.issue_tokens_for_profile(profile)
    }

    /// Refreshes an access token using a valid refresh token (`POST /api/v1/auth/refresh`).
    pub async fn refresh_tokens(&self, refresh_token: &str) -> CoreResult<TokenPairResponse> {
        let claims =
            decode_and_verify_token(refresh_token, &self.jwt_secret, AuthTokenKind::Refresh)?;

        let users = self.users_by_id.read().await;
        let user = users.get(&claims.sub).ok_or_else(|| {
            AdbHubError::WindowsUsbError("User no longer exists".to_string())
        })?;
        if user.status != UserStatus::Active {
            return Err(AdbHubError::WindowsUsbError(
                "User account is suspended or disabled".to_string(),
            ));
        }

        let profile = PublicUserProfile::from(user);
        drop(users);

        self.issue_tokens_for_profile(profile)
    }

    /// Verifies a Bearer access token from an HTTP Authorization header.
    pub fn verify_access_token(&self, access_token: &str) -> CoreResult<AuthTokenClaims> {
        decode_and_verify_token(access_token, &self.jwt_secret, AuthTokenKind::Access)
    }

    /// Returns total registered user count.
    pub async fn user_count(&self) -> usize {
        self.users_by_id.read().await.len()
    }

    fn issue_tokens_for_profile(&self, user: PublicUserProfile) -> CoreResult<TokenPairResponse> {
        let now = Utc::now();
        let access_exp = (now + Duration::hours(1)).timestamp();
        let refresh_exp = (now + Duration::days(7)).timestamp();

        let access_claims = AuthTokenClaims {
            sub: user.id,
            email: user.email.clone(),
            token_type: AuthTokenKind::Access,
            jti: Uuid::new_v4(),
            iat: now.timestamp(),
            exp: access_exp,
        };
        let refresh_claims = AuthTokenClaims {
            sub: user.id,
            email: user.email.clone(),
            token_type: AuthTokenKind::Refresh,
            jti: Uuid::new_v4(),
            iat: now.timestamp(),
            exp: refresh_exp,
        };

        let access_token = encode_signed_token(&access_claims, &self.jwt_secret)?;
        let refresh_token = encode_signed_token(&refresh_claims, &self.jwt_secret)?;

        Ok(TokenPairResponse {
            access_token,
            refresh_token,
            expires_in_seconds: 3600,
            user,
        })
    }
}
