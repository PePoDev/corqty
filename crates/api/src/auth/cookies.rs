use axum::http::{HeaderMap, HeaderValue, header};

use crate::{config::ApiConfig, error::ApiError};

pub const SESSION_COOKIE: &str = "corqty_session";
pub const CSRF_COOKIE: &str = "corqty_csrf";

pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(str::to_owned))
}

pub fn append_auth_cookies(
    headers: &mut HeaderMap,
    config: &ApiConfig,
    session: &str,
    csrf: &str,
) -> Result<(), ApiError> {
    let secure = if config.secure_cookies {
        "; Secure"
    } else {
        ""
    };
    for value in [
        format!(
            "{SESSION_COOKIE}={session}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
            config.session_days * 86_400
        ),
        format!(
            "{CSRF_COOKIE}={csrf}; Path=/; SameSite=Lax; Max-Age={}{secure}",
            config.session_days * 86_400
        ),
    ] {
        headers.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&value).map_err(ApiError::internal)?,
        );
    }
    Ok(())
}

pub fn clear_auth_cookies(headers: &mut HeaderMap, config: &ApiConfig) {
    let secure = if config.secure_cookies {
        "; Secure"
    } else {
        ""
    };
    for name in [SESSION_COOKIE, CSRF_COOKIE] {
        let value = format!("{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{secure}");
        headers.append(
            header::SET_COOKIE,
            HeaderValue::from_str(&value).expect("valid clear-cookie header"),
        );
    }
}
