//! Talos Terrace desktop entry point.
//!
//! M3 adds `terrace mcp`, the headless MCP bridge, as a second entry point over the same
//! library; it will branch here on the first argument.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    terrace_lib::augment_path();
    terrace_lib::run();
}
