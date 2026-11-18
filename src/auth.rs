//! Konten: Anmeldung mit Benutzername und Passwort (Argon2id), Sitzung als HttpOnly-Cookie.
//! Das erste Konto wird Admin; danach legt der Admin weitere Konten an oder öffnet die
//! Registrierung.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use nrztube_types::Me;
use rand::RngCore;

use crate::AppState;

pub const COOKIE: &str = "nrztube_session";

pub fn hash(pw: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default().hash_password(pw.as_bytes(), &salt).map(|h| h.to_string()).unwrap_or_default()
}

pub fn verify(pw: &str, hash: &str) -> bool {
    PasswordHash::new(hash).map(|h| Argon2::default().verify_password(pw.as_bytes(), &h).is_ok()).unwrap_or(false)
}

pub fn new_token() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

pub fn session_cookie(token: &str, secure: bool) -> String {
    format!(
        "{COOKIE}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        90 * 86400,
        if secure { "; Secure" } else { "" }
    )
}

pub fn clear_cookie() -> String {
    format!("{COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

pub fn token_from(parts: &Parts) -> Option<String> {
    let cookies = parts.headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|c| {
        let (k, v) = c.trim().split_once('=')?;
        (k == COOKIE).then(|| v.to_string())
    })
}

/// Angemeldetes Konto – sonst 401
pub struct User(pub Me);

impl FromRequestParts<AppState> for User {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = token_from(parts);
        match token.and_then(|t| state.db.session_user(&t)) {
            Some(me) => Ok(User(me)),
            None => Err((StatusCode::UNAUTHORIZED, axum::Json(serde_json::json!({ "error": "Nicht angemeldet" }))).into_response()),
        }
    }
}

/// Nur für Admins
pub struct Admin(pub Me);

impl FromRequestParts<AppState> for Admin {
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let User(me) = User::from_request_parts(parts, state).await?;
        if !me.admin {
            return Err((StatusCode::FORBIDDEN, axum::Json(serde_json::json!({ "error": "Nur für Admins" }))).into_response());
        }
        Ok(Admin(me))
    }
}
