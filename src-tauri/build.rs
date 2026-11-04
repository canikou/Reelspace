fn main() {
    println!("cargo:rerun-if-env-changed=REELSPACE_YOUTUBE_CLIENT_ID");
    tauri_build::build();
}
