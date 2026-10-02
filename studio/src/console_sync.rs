use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Snapshot {
    pub developer_id: String,
    pub apps: Vec<crate::play_console::ConsoleApp>,
}

pub(crate) struct SyncJob {
    receiver: Receiver<Result<Snapshot, String>>,
    cancelled: Arc<AtomicBool>,
    received: bool,
}

impl SyncJob {
    pub(crate) fn start_with_cookies(
        ctx: &egui::Context,
        console_url: &str,
        cookies: &str,
    ) -> Result<Self, String> {
        let connection = crate::console_cookies::Connection::parse(console_url, cookies)?;
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (sender, receiver) = mpsc::channel();
        let repaint_context = ctx.clone();
        thread::spawn(move || {
            let result = connection.fetch(&worker_cancelled);
            let _ = sender.send(result);
            repaint_context.request_repaint();
        });
        Ok(Self {
            receiver,
            cancelled,
            received: false,
        })
    }

    pub(crate) fn poll(&mut self) -> Option<Result<Snapshot, String>> {
        if self.received {
            return None;
        }
        match self.receiver.try_recv() {
            Ok(result) => {
                self.received = true;
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.received = true;
                Some(Err("Play Console sync stopped unexpectedly.".to_owned()))
            }
        }
    }
}

impl Drop for SyncJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}
