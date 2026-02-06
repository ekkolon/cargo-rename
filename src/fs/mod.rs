//! File system operations with transaction support.
//!
//! Provides atomic file and directory operations that can be committed
//! or rolled back as a unit.

pub mod transaction;

pub use transaction::{Operation, Transaction, TransactionStats};
