use crate::error::{app_error, user_error, AppResult, ErrorCode};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use url::Url;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub(crate) fn open_external_url(url: &str, failure_message: &str) -> AppResult<()> {
    validate_external_url(url)?;
    let operation = wide("open");
    let target = wide(url);
    let result = unsafe {
        ShellExecuteW(
            ptr::null_mut(),
            operation.as_ptr(),
            target.as_ptr(),
            ptr::null(),
            ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    if result as isize > 32 {
        Ok(())
    } else {
        Err(
            app_error(ErrorCode::WindowsIntegrationFailed, failure_message)
                .with_detail(format!("ShellExecuteW returned {}", result as isize)),
        )
    }
}

fn validate_external_url(value: &str) -> AppResult<()> {
    if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
        return Err(user_error("the external browser link is malformed"));
    }
    let parsed =
        Url::parse(value).map_err(|_| user_error("the external browser link is malformed"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return Err(user_error(
            "the external browser link is not a safe HTTPS URL",
        ));
    }
    Ok(())
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_browser_links_require_safe_https_urls() {
        assert!(
            validate_external_url("https://accounts.google.com/o/oauth2/v2/auth?state=abc").is_ok()
        );
        assert!(validate_external_url("https://www.youtube.com/watch?v=abc").is_ok());
        for invalid in [
            "",
            "http://accounts.google.com/",
            "file:///C:/Windows/System32/calc.exe",
            "https://user:password@example.com/",
            "https://example.com/\nnext",
            "not a url",
        ] {
            assert!(
                validate_external_url(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn wide_strings_are_null_terminated_without_changing_text() {
        let encoded = wide("open");
        assert_eq!(encoded.last(), Some(&0));
        assert_eq!(
            String::from_utf16(&encoded[..encoded.len() - 1]).unwrap(),
            "open"
        );
    }
}
