use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 协作式取消标志，替代 tokio CancellationToken（当前依赖树未导出该类型）。
#[derive(Clone, Default)]
pub struct CancelFlag(Arc<AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    pub fn child_token(&self) -> Self {
        self.clone()
    }

    pub fn same_as(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
