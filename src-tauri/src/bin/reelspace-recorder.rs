#[cfg(target_os = "linux")]
fn main() {
    std::process::exit(reelspace_lib::recorder::cli::main());
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("reelspace-recorder is only supported on Linux");
    std::process::exit(1);
}
