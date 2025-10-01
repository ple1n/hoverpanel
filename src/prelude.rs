pub use std::sync::Arc;

pub use anyhow::Ok as aok;
use arc_swap::ArcSwap;

pub type ArcSw<T> = Arc<ArcSwap<T>>;
