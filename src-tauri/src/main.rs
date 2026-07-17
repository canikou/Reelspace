fn main() {
    if let Err(error) = reelspace_lib::run() {
        eprintln!("Reelspace could not start: {error}");
    }
}
