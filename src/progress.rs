#[derive(Debug)]
pub enum ProgressMsg {
    Start {
        id: usize,
        name: String,
        total_size: u64,
    },
    Inc {
        id: usize,
        n: u64,
    },
    Finish {
        id: usize,
    },
    Error {
        id: usize,
        msg: String,
    },
}
