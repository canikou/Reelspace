use crate::error::{user_error, AppResult};
use std::path::Path;

pub fn reveal(path: &Path) -> AppResult<()> {
    if !path.is_file() {
        return Err(user_error("file does not exist on disk"));
    }
    let canonical = path.canonicalize()?;
    tauri_plugin_opener::reveal_item_in_dir(canonical)
        .map_err(|error| user_error(format!("could not reveal the file: {error}")))
}
