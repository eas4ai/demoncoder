//! One background reader; terminal input never waits for record formatting.
use std::time::Duration;

use super::{Request, Snapshot};
use crate::workflow::runtime::SharedRuntime;
use tokio::{sync::watch, task::JoinHandle};

pub(crate) struct Poller {
    request: watch::Sender<Option<Request>>,
    output: watch::Receiver<Option<Result<Snapshot, String>>>,
    task: JoinHandle<()>,
}

impl Poller {
    pub fn start(runtime: SharedRuntime) -> Self {
        let (request, mut requests) = watch::channel(None);
        let (output_tx, output) = watch::channel(None);
        let task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(250));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut delivered = None;
            let mut retained_page = None;
            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    changed = requests.changed() => { if changed.is_err() { break; } }
                }
                let requested = *requests.borrow_and_update();
                let page = requested.filter(|request| Some(*request) != delivered);
                let runtime = runtime.clone();
                let result = tokio::task::spawn_blocking(move || runtime.inspection(page)).await;
                match result {
                    Ok(Ok(Some(mut snapshot))) => {
                        if page.is_some() {
                            delivered = page;
                            retained_page = snapshot.page.take();
                        }
                        // Status refreshes may overwrite the watch slot before
                        // the terminal reads it. Keep the requested page until
                        // its request changes so it cannot disappear unread.
                        snapshot.page = retained_page
                            .as_ref()
                            .filter(|page| Some(page.request) == requested)
                            .cloned();
                        if output_tx.send(Some(Ok(snapshot))).is_err() {
                            break;
                        }
                    }
                    Ok(Ok(None)) => {} // Writer owns the record; keep the last view visibly aged.
                    Ok(Err(error)) => {
                        if output_tx.send(Some(Err(format!("{error:#}")))).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = output_tx.send(Some(Err(
                            "Inspection reader failed; state unavailable.".into(),
                        )));
                        break;
                    }
                }
            }
        });
        Self {
            request,
            output,
            task,
        }
    }

    pub fn request(&self, request: Option<Request>) {
        self.request.send_if_modified(|current| {
            if *current == request {
                false
            } else {
                *current = request;
                true
            }
        });
    }

    pub fn take(&mut self) -> Option<Result<Snapshot, String>> {
        if self.output.has_changed().unwrap_or(false) {
            self.output.borrow_and_update().clone()
        } else {
            None
        }
    }
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn status_refresh_keeps_the_requested_page_available() {
        let root = tempfile::tempdir().unwrap();
        let record = crate::inspection::tests::record(root.path());
        let runtime = SharedRuntime::for_test(&root.path().join("record"), record).unwrap();
        let mut reader = Poller::start(runtime);
        let request = Request::default();
        reader.request(Some(request));
        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(2), reader.output.changed())
                .await
                .unwrap()
                .unwrap();
            let snapshot = reader.output.borrow_and_update().clone().unwrap().unwrap();
            assert_eq!(snapshot.page.unwrap().request, request);
        }
    }
}
