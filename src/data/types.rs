//! Webhook types
// TODO: Move and refactor all logic from src/webhooks/types.rs here.

#[derive(Debug, Clone)]
pub enum PoolUpdateType {
    Add,
    Remove,
    Update,
}
