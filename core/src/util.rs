//! Shared utility functions for the application

use anyhow::{bail, Context, Result};

/// Sanitize a string value for safe use in HTTP headers
/// 
/// HTTP header values have restrictions:
/// - Cannot contain control characters (0x00-0x1F)
/// - Cannot contain DEL (0x7F)
/// - Cannot contain newlines, carriage returns, or null bytes
/// - Must be valid UTF-8
/// 
/// # Arguments
/// * `value` - The string to sanitize
/// * `field_name` - Name of the field for error messages
/// 
/// # Returns
/// The sanitized string, or an error if invalid characters are found
pub fn sanitize_for_header(value: &str, field_name: &str) -> Result<String> {
    if value.is_empty() {
        bail!("{} cannot be empty", field_name);
    }

    for (index, ch) in value.char_indices() {
        let byte = ch as u8;
        // Check for control characters, DEL, null, and line breaks
        if (byte <= 0x1F) || byte == 0x7F || ch == '\0' || ch == '\r' || ch == '\n' {
            bail!(
                "{} contains invalid character at position {} (byte value: {:#04x}). \
                Control characters, newlines, carriage returns, and null bytes are not allowed.",
                field_name,
                index,
                byte
            );
        }
    }

    Ok(value.to_string())
}

/// Validate an API key can be used in an Authorization header
/// 
/// This combines sanitization with an actual HeaderValue parse attempt
/// to catch any edge cases that simple character filtering might miss.
/// 
/// # Arguments
/// * `api_key` - The API key to validate
/// 
/// # Returns
/// The sanitized API key, or an error if validation fails
pub fn validate_api_key(api_key: &str) -> Result<String> {
    let trimmed = api_key.trim();
    
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("none") {
        bail!("API key is empty or set to 'none'");
    }

    // First pass: check for obviously invalid characters
    sanitize_for_header(trimmed, "API key")?;

    // Second pass: validate the full header value can be parsed
    let header_value = format!("Bearer {}", trimmed);
    header_value.parse::<reqwest::header::HeaderValue>()
        .with_context(|| {
            format!(
                "API key results in invalid Authorization header. \
                This may indicate the key contains characters not valid for HTTP headers. \
                Key length: {} characters.",
                trimmed.len()
            )
        })?;

    Ok(trimmed.to_string())
}

/// Sanitize a base URL for API requests
/// 
/// Ensures the URL is properly formatted and doesn't contain
/// problematic characters that could cause request failures.
/// 
/// # Arguments
/// * `url` - The URL to sanitize
/// * `field_name` - Name of the field for error messages
/// 
/// # Returns
/// The sanitized URL, or an error if validation fails
pub fn sanitize_base_url(url: &str, field_name: &str) -> Result<String> {
    let trimmed = url.trim();
    
    if trimmed.is_empty() {
        bail!("{} cannot be empty", field_name);
    }

    // Check for URL-encoded characters that shouldn't be present
    // (indicates double-encoding or configuration corruption)
    if trimmed.contains("%2F") || trimmed.contains("%3D") || trimmed.contains("%20") {
        bail!(
            "{} appears to contain URL-encoded characters (e.g., %2F, %3D, %20). \
            This usually indicates double-encoding or configuration corruption. \
            Please verify the URL is not double-encoded.",
            field_name
        );
    }

    // Validate basic URL structure
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        bail!(
            "{} must start with 'http://' or 'https://'. Got: {}",
            field_name,
            trimmed
        );
    }

    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_for_header_valid() {
        assert!(sanitize_for_header("abc123", "test").is_ok());
        assert!(sanitize_for_header("sk-abc123xyz", "test").is_ok());
        assert!(sanitize_for_header("hello world", "test").is_ok());
    }

    #[test]
    fn test_sanitize_for_header_invalid() {
        assert!(sanitize_for_header("abc\n123", "test").is_err());
        assert!(sanitize_for_header("abc\r123", "test").is_err());
        assert!(sanitize_for_header("abc\x00123", "test").is_err());
        assert!(sanitize_for_header("abc\x1f123", "test").is_err());
        assert!(sanitize_for_header("abc\x7f123", "test").is_err());
    }

    #[test]
    fn test_validate_api_key() {
        assert!(validate_api_key("sk-test123").is_ok());
        assert!(validate_api_key("").is_err());
        assert!(validate_api_key("none").is_err());
        assert!(validate_api_key("NONE").is_err());
        assert!(validate_api_key(" \n ").is_err());
    }

    #[test]
    fn test_sanitize_base_url() {
        assert!(sanitize_base_url("https://api.example.com/v1", "url").is_ok());
        assert!(sanitize_base_url("http://localhost:11434/v1", "url").is_ok());
    }

    #[test]
    fn test_sanitize_base_url_invalid() {
        assert!(sanitize_base_url("", "url").is_err());
        assert!(sanitize_base_url("invalid-url", "url").is_err());
        assert!(sanitize_base_url("https://api.example%2Fcom", "url").is_err());
    }
}
