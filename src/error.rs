use serde::Serialize;

/// Safe text only. Never source errors, headers, cookies, or signed URLs.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppError {
    pub code: &'static str,
    pub message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_status: Option<u16>,
    #[serde(skip)]
    pub hint: &'static str,
    #[serde(skip)]
    pub exit_code: u8,
}

impl AppError {
    pub fn new(code: &'static str, message: &'static str, hint: &'static str) -> Self {
        Self {
            code,
            message,
            http_status: None,
            hint,
            exit_code: 2,
        }
    }

    pub fn http(
        code: &'static str,
        message: &'static str,
        hint: &'static str,
        status: u16,
    ) -> Self {
        let mut error = Self::new(code, message, hint);
        error.http_status = Some(status);
        error.exit_code = 3;
        error
    }
}
