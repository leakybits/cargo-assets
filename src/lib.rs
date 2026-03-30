pub const IO_BUFFER_SIZE: usize = 64 * 1024; // 64 KiB

pub mod cli;
mod download;
pub mod error;
mod metadata;
mod progress;
