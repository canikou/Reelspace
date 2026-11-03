//! Linux recorder daemon: supervises gpu-screen-recorder (replay buffer + rolling/session recording).
pub mod cli;
pub mod clip;
pub mod config;
pub mod control;
pub mod daemon;
pub mod dbus;
pub mod files;
pub mod games;
