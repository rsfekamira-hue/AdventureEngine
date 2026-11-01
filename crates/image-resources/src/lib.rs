//! Image loading policy and value messages. No filesystem, executor, UI or GPU dependencies.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestId(pub u64);

#[derive(Debug)]
pub struct DecodedImage {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
}
impl DecodedImage {
    pub fn validate(&self) -> Result<(), String> {
        let expected = (self.size[0] as usize)
            .checked_mul(self.size[1] as usize)
            .and_then(|n| n.checked_mul(4));
        if self.size.contains(&0) || expected != Some(self.rgba.len()) {
            return Err("Invalid RGBA image dimensions".into());
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct ImageRequest {
    pub id: RequestId,
    pub path: String,
}
#[derive(Debug)]
pub struct ImageResult {
    pub id: RequestId,
    pub result: Result<DecodedImage, String>,
}
/// Registration means queued for ordered rendering, not a CPU wait for GPU completion.
#[derive(Debug)]
pub enum TextureCommand {
    Register { path: String, image: DecodedImage },
    Release { path: String },
}

struct Resident {
    bytes: usize,
    touched: u64,
}
/// One outstanding request bounds decoded results and lets current-scene work take priority.
/// Each owner has its own loader/result mailbox; request IDs reject stale/duplicate replies.
pub struct ImageManager {
    budget: usize,
    required: BTreeSet<String>,
    predicted: Vec<String>,
    resident: BTreeMap<String, Resident>,
    pending: Option<ImageRequest>,
    failures: BTreeMap<String, String>,
    skipped: BTreeSet<String>,
    serial: u64,
    clock: u64,
    commands: Vec<TextureCommand>,
}
impl ImageManager {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            required: BTreeSet::new(),
            predicted: Vec::new(),
            resident: BTreeMap::new(),
            pending: None,
            failures: BTreeMap::new(),
            skipped: BTreeSet::new(),
            serial: 0,
            clock: 0,
            commands: Vec::new(),
        }
    }
    pub fn set_targets(&mut self, required: BTreeSet<String>, predicted: Vec<String>) {
        let predicted = if self.budget == 0 {
            Vec::new()
        } else {
            predicted
        };
        if self.required != required || self.predicted != predicted {
            self.skipped.clear();
            self.required = required;
            self.predicted = predicted;
        }
        self.clock += 1;
        for name in &self.required {
            if let Some(image) = self.resident.get_mut(name) {
                image.touched = self.clock;
            }
        }
        self.trim(0, false);
    }
    fn wanted(&self, path: &str) -> bool {
        self.required.contains(path) || self.predicted.iter().any(|p| p == path)
    }
    pub fn next_request(&mut self) -> Option<ImageRequest> {
        if self.pending.is_some() {
            return None;
        }
        let path = self
            .required
            .iter()
            .chain(self.predicted.iter())
            .find(|p| {
                !self.resident.contains_key(*p)
                    && !self.failures.contains_key(*p)
                    && !self.skipped.contains(*p)
            })?
            .clone();
        self.serial += 1;
        let id = RequestId(self.serial);
        self.pending = Some(ImageRequest {
            id,
            path: path.clone(),
        });
        Some(ImageRequest { id, path })
    }
    pub fn accept(&mut self, reply: ImageResult) {
        if self.pending.as_ref().map(|r| r.id) != Some(reply.id) {
            return;
        }
        let request = self.pending.take().expect("matching pending request");
        if !self.wanted(&request.path) {
            return;
        }
        match reply.result.and_then(|image| {
            image.validate()?;
            Ok(image)
        }) {
            Err(error) => {
                self.failures.insert(request.path, error);
            }
            Ok(image) => {
                let bytes = image.rgba.len();
                let required = self.required.contains(&request.path);
                // Never evict the current prediction window merely to cycle through it again.
                if !required && (bytes > self.budget || !self.can_fit_prediction(bytes)) {
                    self.skipped.insert(request.path);
                    return;
                }
                self.trim(bytes, !required);
                self.resident.insert(
                    request.path.clone(),
                    Resident {
                        bytes,
                        touched: self.clock,
                    },
                );
                self.commands.push(TextureCommand::Register {
                    path: request.path,
                    image,
                });
            }
        }
    }
    fn can_fit_prediction(&self, bytes: usize) -> bool {
        let protected: usize = self
            .resident
            .iter()
            .filter(|(p, _)| self.wanted(p))
            .map(|(_, image)| image.bytes)
            .sum();
        protected.saturating_add(bytes) <= self.budget
    }
    fn trim(&mut self, incoming: usize, protect_predictions: bool) {
        while self.resident_bytes().saturating_add(incoming) > self.budget {
            let victim = self
                .resident
                .iter()
                .filter(|(p, _)| {
                    !self.required.contains(*p)
                        && !(protect_predictions && self.predicted.contains(p))
                })
                .min_by_key(|(_, image)| image.touched)
                .map(|(p, _)| p.clone());
            let Some(path) = victim else {
                break;
            }; // Required images may exceed the budget.
            self.resident.remove(&path);
            self.skipped.insert(path.clone());
            self.commands.push(TextureCommand::Release { path });
        }
    }
    pub fn take_commands(&mut self) -> Vec<TextureCommand> {
        std::mem::take(&mut self.commands)
    }
    pub fn failures(&self) -> &BTreeMap<String, String> {
        &self.failures
    }
    pub fn waiting(&self) -> bool {
        self.required
            .iter()
            .any(|p| !self.resident.contains_key(p) && !self.failures.contains_key(p))
    }
    pub fn resident_bytes(&self) -> usize {
        self.resident.values().map(|r| r.bytes).sum()
    }
    pub fn pending_path(&self) -> Option<&str> {
        self.pending.as_ref().map(|r| r.path.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn names(values: &[&str]) -> BTreeSet<String> {
        values.iter().map(|v| v.to_string()).collect()
    }
    fn finish(manager: &mut ImageManager, id: RequestId) {
        manager.accept(ImageResult {
            id,
            result: Ok(DecodedImage {
                size: [1, 1],
                rgba: vec![255; 4],
            }),
        });
    }
    #[test]
    fn prioritizes_required_and_discards_old_and_duplicate_results() {
        let mut m = ImageManager::new(8);
        m.set_targets(names(&["current"]), vec!["next".into()]);
        let first = m.next_request().unwrap();
        assert_eq!(first.path, "current");
        assert!(m.next_request().is_none());
        finish(&mut m, RequestId(99));
        assert!(m.waiting());
        m.set_targets(names(&["other"]), vec![]);
        finish(&mut m, first.id);
        assert!(m.take_commands().is_empty());
        let second = m.next_request().unwrap();
        assert_eq!(second.path, "other");
        finish(&mut m, second.id);
        finish(&mut m, second.id);
        assert_eq!(m.take_commands().len(), 1);
        assert!(!m.waiting());
    }
    #[test]
    fn cache_is_bounded_without_prediction_churn_and_protects_required() {
        let mut m = ImageManager::new(4);
        m.set_targets(names(&["a"]), vec!["b".into()]);
        let a = m.next_request().unwrap();
        finish(&mut m, a.id);
        let b = m.next_request().unwrap();
        finish(&mut m, b.id);
        assert!(m.next_request().is_none());
        assert_eq!(m.resident_bytes(), 4);
        m.set_targets(names(&["a", "b"]), vec![]);
        let b = m.next_request().unwrap();
        finish(&mut m, b.id);
        assert_eq!(m.resident_bytes(), 8);
        m.set_targets(names(&["b"]), vec![]);
        assert_eq!(m.resident_bytes(), 4);
        assert!(m
            .take_commands()
            .iter()
            .any(|c| matches!(c, TextureCommand::Release { path } if path == "a")));
    }
    #[test]
    fn invalid_and_failed_images_do_not_retry_forever() {
        let mut m = ImageManager::new(4);
        m.set_targets(names(&["bad"]), vec![]);
        let r = m.next_request().unwrap();
        m.accept(ImageResult {
            id: r.id,
            result: Ok(DecodedImage {
                size: [2, 2],
                rgba: vec![],
            }),
        });
        assert!(m.next_request().is_none());
        assert!(m.failures().contains_key("bad"));
    }
}
