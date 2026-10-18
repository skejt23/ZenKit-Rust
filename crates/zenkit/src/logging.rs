// Copyright (c) 2026 skejt23 (ZenKit Rust port)
// Copyright (c) 2023 GothicKit Contributors.
// SPDX-License-Identifier: MIT
//! Optional, process-wide diagnostic messages.
//!
//! Logging starts disabled. Applications can install a callback with [`set`],
//! select the best-effort stderr sink with [`set_default`], or [`disable`] it.
//! Callbacks can forward records to an application's logging facade.
//! Operation errors and parser diagnostics are also returned through their APIs.
//!
//! Callbacks run on emitting threads and may run concurrently. Copy messages and
//! targets to retain them. No configuration lock is held during formatting,
//! callbacks or callback destruction. Replacing a hook does not cancel emissions
//! already in flight.
//!
//! Callbacks may replace the hook or invoke library operations recursively. Callback and formatter
//! panics propagate without poisoning the configuration lock; they may leave
//! partial operation effects. A replaced callback is dropped after installing
//! the new hook. Callback work is outside the library's resource limits.
//!
//! ```
//! use zenkit::logging::{self, Level};
//! logging::set(Level::Warn, |record| {
//!     eprintln!("{} {}: {}", record.level, record.target, record.args);
//! });
//! // Library operations now forward their warnings and errors to the callback.
//! logging::disable();
//! ```

use std::{
    fmt,
    io::Write,
    sync::{Arc, RwLock},
};

/// Severity in increasing verbosity order. A cutoff includes all lower levels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl fmt::Display for Level {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Error => "ERROR",
            Self::Warn => "WARN",
            Self::Info => "INFO",
            Self::Debug => "DEBUG",
            Self::Trace => "TRACE",
        })
    }
}

/// One synchronously borrowed message. Formatting is deferred until consumed.
#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    pub level: Level,
    pub target: &'a str,
    pub args: fmt::Arguments<'a>,
}

type Callback = dyn for<'a> Fn(Record<'a>) + Send + Sync + 'static;
struct Configuration {
    cutoff: Level,
    callback: Option<Arc<Callback>>,
}
static CONFIGURATION: RwLock<Configuration> = RwLock::new(Configuration {
    cutoff: Level::Info,
    callback: None,
});

fn replace(cutoff: Level, callback: Option<Arc<Callback>>) {
    let previous = {
        let mut configuration = CONFIGURATION.write().unwrap_or_else(|e| e.into_inner());
        std::mem::replace(&mut *configuration, Configuration { cutoff, callback })
    };
    drop(previous);
}

/// Replace the callback and its inclusive severity cutoff together.
///
/// The callback is shared between emitting threads. Use synchronization for
/// mutable captured state. Replacement and callback destruction may reenter this
/// module; an in-flight emission retains its own snapshot and may still invoke
/// the previous callback after replacement returns.
pub fn set(cutoff: Level, callback: impl for<'a> Fn(Record<'a>) + Send + Sync + 'static) {
    replace(cutoff, Some(Arc::new(callback)));
}

/// Stop accepting new messages. In-flight emissions may still invoke or finish
/// their previously selected callback.
pub fn disable() {
    replace(Level::Info, None);
}

/// Install a best-effort stderr sink with the selected inclusive cutoff.
///
/// Lines contain the level, target and full formatted message. This plain-text
/// format has no stable color-code or timestamp representation.
/// Stderr I/O failures are ignored; formatter panics still propagate.
pub fn set_default(cutoff: Level) {
    set(cutoff, |record| {
        let _ = writeln!(
            std::io::stderr().lock(),
            "[ZenKit] {} {}: {}",
            record.level,
            record.target,
            record.args
        );
    });
}

/// Whether a message would currently be accepted. This is a snapshot: another
/// thread may replace the configuration before the subsequent [`emit`] call.
/// Use this as an optional guard for expensive argument construction.
pub(crate) fn enabled(level: Level) -> bool {
    let configuration = CONFIGURATION.read().unwrap_or_else(|e| e.into_inner());
    configuration.callback.is_some() && level <= configuration.cutoff
}

/// Emit one message through a consistent callback/cutoff snapshot.
///
/// Disabled messages never invoke a message's `Display` implementation. Argument
/// expressions are evaluated before this function is called; [`enabled`] can
/// guard expensive argument construction. Message storage is not truncated.
pub(crate) fn emit(level: Level, target: &str, args: fmt::Arguments<'_>) {
    let callback = {
        let configuration = CONFIGURATION.read().unwrap_or_else(|e| e.into_inner());
        if level > configuration.cutoff {
            return;
        }
        configuration.callback.clone()
    };
    if let Some(callback) = callback {
        callback(Record {
            level,
            target,
            args,
        });
    }
}
