use crate::error::{user_error, AppResult};
use std::path::Path;
use std::process::Command;

pub fn reveal(path: &Path) -> AppResult<()> {
    if !path.exists() {
        return Err(user_error("file does not exist on disk"));
    }
    #[cfg(windows)]
    {
        Command::new("explorer")
            .arg(format!("/select,{}", path.to_string_lossy()))
            .spawn()?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let parent = path.parent().ok_or_else(|| user_error("file has no parent folder"))?;
        opener::open(parent)?;
        Ok(())
    }
}
