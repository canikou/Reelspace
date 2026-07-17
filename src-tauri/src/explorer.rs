use crate::error::{app_error, AppResult, ErrorCode};
use std::path::Path;
use std::process::Command;

pub fn reveal(path: &Path) -> AppResult<()> {
    if !path.is_file() {
        return Err(app_error(
            ErrorCode::FileNotFound,
            "The file does not exist on disk",
        ));
    }
    let canonical = path.canonicalize()?;
    let argument = format!("/select,{}", canonical.to_string_lossy());
    let status = Command::new("explorer.exe")
        .arg(argument)
        .status()
        .map_err(|error| {
            app_error(
                ErrorCode::WindowsIntegrationFailed,
                "Windows Explorer could not be opened",
            )
            .with_detail(error.to_string())
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(app_error(
            ErrorCode::WindowsIntegrationFailed,
            "Windows Explorer could not reveal the file",
        ))
    }
}
