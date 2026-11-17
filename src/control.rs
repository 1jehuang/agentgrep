//! Cooperative resource limits for synchronous searches.
//!
//! Install the scope *inside* a blocking worker. Setting the shared token to
//! true cancels that search. Scopes restore their predecessor, even on unwind.
use std::cell::RefCell;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// Finite budgets for one scoped search. Byte budgets count raw input, before
/// UTF-8 conversion. Output budgets apply independently to each rg invocation.
#[derive(Debug, Clone)]
pub struct SearchLimits {
    pub timeout: Duration,
    pub max_output_bytes: usize,
    pub max_stderr_bytes: usize,
    pub max_file_bytes: usize,
    pub max_total_read_bytes: usize,
    pub max_entries: usize,
    pub max_matches: usize,
    pub max_structure_items: usize,
    pub max_line_bytes: usize,
    pub max_file_lines: usize,
}
impl Default for SearchLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_output_bytes: 16 * 1024 * 1024,
            max_stderr_bytes: 1024 * 1024,
            max_file_bytes: 8 * 1024 * 1024,
            max_total_read_bytes: 64 * 1024 * 1024,
            max_entries: 100_000,
            max_matches: 100_000,
            max_structure_items: 100_000,
            max_line_bytes: 1024 * 1024,
            max_file_lines: 100_000,
        }
    }
}
#[derive(Debug, Default)]
struct Usage {
    error: Option<String>,
    read: usize,
    entries: usize,
    matches: usize,
    items: usize,
}
#[derive(Debug, Clone)]
pub struct SearchControl {
    token: Arc<AtomicBool>,
    limits: SearchLimits,
    started: Instant,
    usage: Arc<Mutex<Usage>>,
}
impl SearchControl {
    /// Starts a new search deadline now. Clones share cancellation and usage,
    /// allowing native workers to consume the same aggregate budgets.
    pub fn new(token: Arc<AtomicBool>, limits: SearchLimits) -> Self {
        Self {
            token,
            limits,
            started: Instant::now(),
            usage: Arc::default(),
        }
    }
}

#[cfg(test)]
#[path = "control_baseline.rs"]
mod baseline;
#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;

thread_local! { static CONTROL: RefCell<Option<SearchControl>> = const { RefCell::new(None) }; }
struct Restore(Option<SearchControl>);
impl Drop for Restore {
    fn drop(&mut self) {
        CONTROL.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}
/// Runs `f` under shared cancellation and finite limits. Resource failure takes
/// precedence over the closure's value (including a partial result). A closure
/// returning Result produces a nested Result. Controls are per search, not reusable.
pub fn with_search_control<T>(control: SearchControl, f: impl FnOnce() -> T) -> Result<T, String> {
    let previous = CONTROL.with(|slot| slot.replace(Some(control)));
    let _restore = Restore(previous);
    checkpoint()?;
    let result = f();
    checkpoint()?;
    Ok(result)
}
// Only internal leaf operations may run inside this borrow. In particular, never
// invoke a user closure or install/restore a scope while it is alive.
fn with_current<T>(f: impl FnOnce(Option<&SearchControl>) -> T) -> T {
    CONTROL.with(|slot| f(slot.borrow().as_ref()))
}
pub(crate) fn current() -> Option<SearchControl> {
    CONTROL.with(|s| s.borrow().clone())
}
pub(crate) fn scoped<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    if with_current(|control| control.is_some()) {
        checkpoint()?;
        let value = f();
        checkpoint()?;
        value
    } else {
        with_search_control(
            SearchControl::new(Arc::new(AtomicBool::new(false)), SearchLimits::default()),
            f,
        )?
    }
}
pub(crate) fn inherited<T: Default>(control: Option<SearchControl>, f: impl FnOnce() -> T) -> T {
    match control {
        Some(control) => with_search_control(control, f).unwrap_or_default(),
        None => f(),
    }
}
pub(crate) fn limits() -> SearchLimits {
    with_current(|control| control.map(|c| c.limits.clone()).unwrap_or_default())
}
pub(crate) fn fail(message: impl Into<String>) -> String {
    let message = message.into();
    with_current(|control| {
        if let Some(c) = control {
            let mut usage = c.usage.lock().unwrap();
            return usage.error.get_or_insert(message).clone();
        }
        message
    })
}
pub(crate) fn checkpoint() -> Result<(), String> {
    with_current(|control| {
        if let Some(c) = control {
            if let Some(error) = c.usage.lock().unwrap().error.clone() {
                return Err(error);
            }
            if c.token.load(Ordering::Relaxed) {
                return Err(fail("search cancelled"));
            }
            if c.started.elapsed() >= c.limits.timeout {
                return Err(fail("search deadline exceeded"));
            }
        }
        Ok(())
    })
}
pub(crate) fn charge(kind: &str, amount: usize) -> Result<(), String> {
    checkpoint()?;
    with_current(|control| {
        if let Some(c) = control {
            let mut usage = c.usage.lock().unwrap();
            let (counter, limit) = match kind {
                "read bytes" => (&mut usage.read, c.limits.max_total_read_bytes),
                "entries" => (&mut usage.entries, c.limits.max_entries),
                "structure items" => (&mut usage.items, c.limits.max_structure_items),
                _ => (&mut usage.matches, c.limits.max_matches),
            };
            *counter = counter.saturating_add(amount);
            if *counter > limit {
                let error = format!("search resource limit exceeded: {kind} (limit {limit})");
                usage.error = Some(error.clone());
                return Err(error);
            }
        }
        Ok(())
    })
}
