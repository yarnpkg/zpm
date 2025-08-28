use std::sync::Arc;

use http::Method;
use reqwest::Response;

use crate::{config::Config, error::Error, http::{HttpClient, HttpRequest, HttpRequestParams}};

fn is_otp_error(response: &Response) -> bool {
    let www_authenticate
        = response.headers()
            .get("www-authenticate");

    if let Some(www_authenticate) = www_authenticate {
        if let Ok(www_authenticate_value) = www_authenticate.to_str() {
            return www_authenticate_value.split(",").any(|s| s.trim() == "otp");
        }
    }

    false
}
