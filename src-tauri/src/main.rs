#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = reelspace_lib::run() {
        eprintln!("Reelspace could not start: {error}");
    }
}
