#[derive(Debug)]
pub enum Progress {
    Start { id: usize, name: String, size: u64 },
    Reset { id: usize, name: String, size: u64 },
    Inc { id: usize, n: u64 },
    Finish { id: usize },
    Error { id: usize, msg: String },
}
