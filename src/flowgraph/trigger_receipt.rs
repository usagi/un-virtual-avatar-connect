//! Per-request observation of selected exec outputs. No node data is retained.
use std::collections::BTreeSet;
use std::sync::Mutex;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptPhase { Pending, Running, Finished, Failed }

#[derive(Debug, Clone, Serialize)]
pub struct ReceiptSnapshot {
    pub phase: ReceiptPhase,
    pub generation: Option<u64>,
    pub outputs: BTreeSet<(String, String)>,
}

/// Owned by the exact trigger and its caller, never looked up by latest node.
/// Finished means this trigger's synchronous exec chain ended, not that a file
/// was saved. Callers must inspect their designated persistence output ports.
#[derive(Debug)]
pub struct TriggerReceipt {
    watched: BTreeSet<(String, String)>,
    state: Mutex<ReceiptSnapshot>,
}
impl TriggerReceipt {
    pub fn new(watched: Vec<(String, String)>) -> Result<Self, &'static str> {
        if watched.is_empty() || watched.len() > 16 || watched.iter().any(|(n,p)| n.is_empty() || n.len()>512 || p.is_empty() || p.len()>128) {
            return Err("receipt requires 1..16 bounded output port identities");
        }
        Ok(Self { watched: watched.into_iter().collect(), state: Mutex::new(ReceiptSnapshot { phase: ReceiptPhase::Pending, generation: None, outputs: BTreeSet::new() }) })
    }
    pub fn snapshot(&self) -> Option<ReceiptSnapshot> { self.state.lock().ok().map(|s| s.clone()) }
    /// Timeout is an observation boundary, never cancellation or success.
    pub async fn wait_snapshot(&self, wait: std::time::Duration) -> Option<ReceiptSnapshot> {
        let deadline=tokio::time::Instant::now()+wait.min(std::time::Duration::from_secs(2));
        loop {
            let snapshot=self.snapshot()?;
            if matches!(snapshot.phase,ReceiptPhase::Finished|ReceiptPhase::Failed) || tokio::time::Instant::now()>=deadline {
                return Some(snapshot);
            }
            tokio::time::sleep_until(deadline.min(tokio::time::Instant::now()+std::time::Duration::from_millis(10))).await;
        }
    }
    pub(crate) fn begin(&self, generation: u64) {
        if let Ok(mut s)=self.state.lock() { s.phase=ReceiptPhase::Running; s.generation=Some(generation); }
    }
    pub(crate) fn observe(&self, node: &str, port: &str) {
        let key=(node.to_owned(),port.to_owned());
        if self.watched.contains(&key) {
            if let Ok(mut s)=self.state.lock() { if s.phase==ReceiptPhase::Running { s.outputs.insert(key); } }
        }
    }
    pub(crate) fn finish(&self, failed: bool) {
        if let Ok(mut s)=self.state.lock() { s.phase=if failed { ReceiptPhase::Failed } else { ReceiptPhase::Finished }; }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn receipt_timeout_remains_pending_and_does_not_cancel() {
        let receipt=TriggerReceipt::new(vec![("save".into(),"on_success".into())]).unwrap();
        assert_eq!(receipt.wait_snapshot(std::time::Duration::ZERO).await.unwrap().phase,ReceiptPhase::Pending);
        receipt.begin(9);receipt.observe("save","on_success");receipt.finish(false);
        assert_eq!(receipt.wait_snapshot(std::time::Duration::ZERO).await.unwrap().phase,ReceiptPhase::Finished);
    }
    #[test]
    fn receipt_is_request_local_and_bounded() {
        let a=TriggerReceipt::new(vec![("save".into(),"on_success".into())]).unwrap();
        let b=TriggerReceipt::new(vec![("save".into(),"on_success".into())]).unwrap();
        a.begin(7); b.begin(8); a.observe("save","on_success"); a.observe("other","on_success"); a.finish(false);
        b.finish(true);
        assert_eq!(a.snapshot().unwrap().outputs.len(),1);
        assert!(b.snapshot().unwrap().outputs.is_empty());
        assert_eq!(b.snapshot().unwrap().phase,ReceiptPhase::Failed);
        a.observe("save","on_success");
        assert_eq!(a.snapshot().unwrap().generation,Some(7));
        assert!(TriggerReceipt::new(vec![("save".into(),"on_success".into());17]).is_err());
        assert!(TriggerReceipt::new(vec![]).is_err());
    }
}
