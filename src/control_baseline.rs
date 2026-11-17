//! Frozen hot functions from a2c52bb, for paired default-dev-profile measurement.
use super::*;

fn current() -> Option<SearchControl> {
    CONTROL.with(|s| s.borrow().clone())
}

pub(super) fn limits() -> SearchLimits {
    current().map(|c| c.limits).unwrap_or_default()
}
pub(super) fn fail(message: impl Into<String>) -> String {
    let message = message.into();
    if let Some(c) = current() {
        let mut usage = c.usage.lock().unwrap();
        return usage.error.get_or_insert(message).clone();
    }
    message
}
pub(super) fn checkpoint() -> Result<(), String> {
    if let Some(c) = current() {
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
}
pub(super) fn charge(kind: &str, amount: usize) -> Result<(), String> {
    checkpoint()?;
    if let Some(c) = current() {
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
}
