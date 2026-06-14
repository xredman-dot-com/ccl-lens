/// Approximate public Anthropic pricing in USD per million tokens.
/// (input, output, cache_write, cache_read). Editable as prices change.
fn rates(model: &str) -> (f64, f64, f64, f64) {
    let m = model.to_lowercase();
    if m.contains("opus") {
        (15.0, 75.0, 18.75, 1.5)
    } else if m.contains("haiku") {
        (0.8, 4.0, 1.0, 0.08)
    } else {
        // sonnet and unknown fallback
        (3.0, 15.0, 3.75, 0.3)
    }
}

pub fn cost_usd(
    model: &str,
    input: u64,
    output: u64,
    cache_read: u64,
    cache_creation: u64,
) -> f64 {
    let (ri, ro, rcw, rcr) = rates(model);
    let total = input as f64 * ri
        + output as f64 * ro
        + cache_creation as f64 * rcw
        + cache_read as f64 * rcr;
    total / 1_000_000.0
}
