//! Background media imports. Decoding and peak-pyramid construction can take seconds for a
//! long recording, so neither is allowed to hold up the interface thread.

use crate::audio::{self, AudioData, Peaks};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

pub struct PreparedImport {
    pub data: AudioData,
    pub bits: u16,
    pub float: bool,
    pub peaks: Peaks,
}

pub struct ImportEvent {
    pub id: u64,
    pub path: PathBuf,
    pub result: Result<PreparedImport, String>,
}

pub struct ImportQueue {
    tx: Sender<ImportEvent>,
    rx: Receiver<ImportEvent>,
    pending: HashMap<u64, PathBuf>,
}

impl Default for ImportQueue {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            pending: HashMap::new(),
        }
    }
}

impl ImportQueue {
    pub fn is_pending(&self, id: u64) -> bool {
        self.pending.contains_key(&id)
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn start(&mut self, id: u64, path: PathBuf, ctx: Option<egui::Context>) {
        self.start_with(id, path.clone(), ctx, move || {
            let loaded = audio::load(&path)
                .map_err(|e| format!("Could not read {}: {e}", path.display()))?;
            // Peaks are much larger than a waveform pixel column and are built once, here,
            // rather than while the person is waiting for the first responsive frame.
            let peaks = crate::peakcache::peaks_for(&path, &loaded.data);
            Ok(PreparedImport {
                data: loaded.data,
                bits: loaded.bits,
                float: loaded.float,
                peaks,
            })
        });
    }

    fn start_with(
        &mut self,
        id: u64,
        path: PathBuf,
        ctx: Option<egui::Context>,
        work: impl FnOnce() -> Result<PreparedImport, String> + Send + 'static,
    ) {
        let tx = self.tx.clone();
        self.pending.insert(id, path.clone());
        std::thread::spawn(move || {
            let result = work();
            // Closing a file before its import finishes is harmless: App drops stale events.
            let _ = tx.send(ImportEvent { id, path, result });
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }

    pub fn drain(&mut self) -> Vec<ImportEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.rx.try_recv() {
            self.pending.remove(&event.id);
            events.push(event);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::time::Duration;

    #[test]
    fn worker_finishes_independently_and_peaks_are_ready() {
        let mut q = ImportQueue::default();
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = barrier.clone();
        q.start_with(7, PathBuf::from("long.wav"), None, move || {
            worker_barrier.wait();
            let data = AudioData::new(48000, 2, 48000);
            let peaks = Peaks::build(&data);
            Ok(PreparedImport {
                data,
                bits: 32,
                float: true,
                peaks,
            })
        });
        assert!(q.is_pending(7));
        assert!(
            q.drain().is_empty(),
            "the interface does not wait for decoding"
        );
        barrier.wait();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let events = q.drain();
            if let Some(event) = events.into_iter().next() {
                let prepared = event.result.unwrap_or_else(|e| panic!("{e}"));
                assert_eq!(prepared.data.frames(), 48000);
                assert!(!prepared.peaks.levels.is_empty());
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "worker did not finish"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(q.is_empty());
    }
}
