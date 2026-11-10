pub mod app;
pub mod appearance;
pub mod buffer;
mod buffers;
pub mod command;
pub mod command_history;
pub mod command_registry;
#[cfg(unix)]
pub mod commit;
pub mod completion;
pub mod diff;
pub mod display;
pub mod editing;
mod exit;
pub mod files;
pub mod frame;
mod git;
pub mod history;
pub mod input;
pub mod keymap;
mod messages;
pub mod minibuffer;
pub mod mode;
pub(crate) mod panes;
mod platform;
#[cfg(feature = "native-profiler")]
pub mod preparation_trace;
mod saves;
pub mod scripting;
pub mod section;
pub mod session;
pub mod status;
mod syntax;
#[cfg(test)]
mod test_support;
pub mod text;
pub mod theme;
pub mod view;
