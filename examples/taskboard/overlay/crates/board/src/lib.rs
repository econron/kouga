// Both transports compile the same owner-scoped Board source. The HTTP-only
// endpoint declarations are disabled for this package, so no HTTP dependency
// enters the gRPC binary.
#[path = "../../../src/board.rs"]
mod board;

pub use board::{Board, CountOutput, Project, Task};
