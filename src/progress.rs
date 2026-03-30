use crate::error::Result;
use derive_more::Debug;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::collections::HashMap;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum Progress {
    Start { id: usize, name: String, size: u64 },
    Reset { id: usize, name: String, size: u64 },
    Inc { id: usize, n: u64 },
    Finish { id: usize },
    Error { id: usize, msg: String },
}

#[derive(Debug)]
pub struct ProgressWatcher {
    mp: MultiProgress,

    #[debug(skip)]
    style: ProgressStyle,
}

impl ProgressWatcher {
    pub fn new(style: ProgressStyle) -> Self {
        Self {
            mp: MultiProgress::new(),
            style,
        }
    }

    pub async fn watch(self, mut rx: mpsc::UnboundedReceiver<Progress>) -> Result<()> {
        let mut bars = HashMap::new();

        while let Some(msg) = rx.recv().await {
            match msg {
                Progress::Start { id, name, size } => {
                    let pb = self.mp.add(ProgressBar::new(size as _));
                    pb.set_style(self.style.clone());
                    pb.set_message(name);
                    bars.insert(id, pb);
                }

                Progress::Reset { id, name, size } => {
                    if let Some(pb) = bars.get(&id) {
                        pb.set_length(size as _);
                        pb.reset();
                        pb.set_message(name);
                    }
                }

                Progress::Inc { id, n } => {
                    if let Some(pb) = bars.get(&id) {
                        pb.inc(n as _);
                    }
                }

                Progress::Finish { id } => {
                    if let Some(pb) = bars.remove(&id) {
                        pb.finish_and_clear();
                    }
                }

                Progress::Error { id, msg } => {
                    if let Some(pb) = bars.remove(&id) {
                        pb.finish_with_message(msg);
                    }
                }
            }
        }

        Ok(())
    }
}
