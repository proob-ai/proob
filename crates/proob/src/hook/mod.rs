//! The Claude Code hook edge. A hook never fails: whatever happens, it prints
//! either nothing or one JSON object and exits 0, so a broken proob never
//! blocks normal work (R12). `session_start` shows where the team stands;
//! `pre_tool_use` is fail-fast schema feedback on writes under `proob/`,
//! never the honesty boundary.
pub mod pre_tool_use;
pub mod session_start;
