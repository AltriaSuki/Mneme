//! Prometheus metrics exporter (feature-gated behind `prometheus`).

use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::PrometheusBuilder;

/// Install the Prometheus recorder and spawn the HTTP listener.
pub fn init(port: u16) -> anyhow::Result<()> {
    PrometheusBuilder::new()
        .with_http_listener(([0, 0, 0, 0], port))
        .install()
        .map_err(|e| anyhow::anyhow!("Prometheus init failed: {}", e))?;
    tracing::info!("Prometheus metrics on :{}", port);
    Ok(())
}

/// Record an LLM API call.
pub fn record_llm_call(provider: &str, latency_ms: u64, success: bool) {
    histogram!("mneme_llm_latency_ms", "provider" => provider.to_string())
        .record(latency_ms as f64);
    counter!("mneme_llm_calls_total", "provider" => provider.to_string(), "status" => if success { "ok" } else { "error" }.to_string())
        .increment(1);
}

/// Snapshot organism state into gauges.
pub fn record_state(energy: f64, stress: f64, valence: f64, arousal: f64) {
    gauge!("mneme_energy").set(energy);
    gauge!("mneme_stress").set(stress);
    gauge!("mneme_affect_valence").set(valence);
    gauge!("mneme_affect_arousal").set(arousal);
}

/// Record token usage.
pub fn record_tokens(input: u64, output: u64) {
    counter!("mneme_tokens_input_total").increment(input);
    counter!("mneme_tokens_output_total").increment(output);
}
