//! Neural modulators: state → ModulationVector mapping.
//!
//! Layer 2a (legacy): Static MLP (5→8→6), blends with ModulationCurves.
//! Layer 2b (ADR-016): Liquid Time-Constant network with persistent hidden state,
//!   dynamic τ, and online Hebbian plasticity (ADR-017).

use crate::somatic::ModulationVector;
use serde::{Deserialize, Serialize};

const INPUT_DIM: usize = 5;
const HIDDEN_DIM: usize = 8;
const OUTPUT_DIM: usize = 6;

/// A small MLP that maps OrganismState dimensions to ModulationVector.
#[derive(Clone, Serialize, Deserialize)]
pub struct NeuralModulator {
    /// Weights: input → hidden (HIDDEN_DIM × INPUT_DIM)
    w1: Vec<Vec<f32>>,
    /// Biases: hidden (HIDDEN_DIM)
    b1: Vec<f32>,
    /// Weights: hidden → output (OUTPUT_DIM × HIDDEN_DIM)
    w2: Vec<Vec<f32>>,
    /// Biases: output (OUTPUT_DIM)
    b2: Vec<f32>,
    /// Blend factor: 0.0 = pure curves, 1.0 = pure neural (gradual transition)
    pub blend: f32,
}

/// Input features extracted from OrganismState.
#[derive(Clone)]
pub struct StateFeatures {
    pub energy: f32,
    pub stress: f32,
    pub arousal: f32,
    pub mood_bias: f32,
    pub social_need: f32,
}

impl StateFeatures {
    fn as_array(&self) -> [f32; INPUT_DIM] {
        [self.energy, self.stress, self.arousal, self.mood_bias, self.social_need]
    }
}

impl NeuralModulator {
    /// Create with small random weights (Xavier-like initialization).
    pub fn new() -> Self {
        use std::f32::consts::SQRT_2;
        let scale1 = SQRT_2 / (INPUT_DIM as f32).sqrt();
        let scale2 = SQRT_2 / (HIDDEN_DIM as f32).sqrt();

        let mut seed: u64 = 42;
        let mut rng = move || -> f32 {
            // Simple xorshift64 PRNG
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            ((seed as f32) / (u64::MAX as f32)) * 2.0 - 1.0
        };

        let w1 = (0..HIDDEN_DIM)
            .map(|_| (0..INPUT_DIM).map(|_| rng() * scale1).collect())
            .collect();
        let b1 = vec![0.0; HIDDEN_DIM];
        let w2 = (0..OUTPUT_DIM)
            .map(|_| (0..HIDDEN_DIM).map(|_| rng() * scale2).collect())
            .collect();
        let b2 = vec![0.0; OUTPUT_DIM];

        Self { w1, b1, w2, b2, blend: 0.0 }
    }

    /// Forward pass: state features → raw output (6 dims).
    fn forward_raw(&self, input: &[f32; INPUT_DIM]) -> ([f32; HIDDEN_DIM], [f32; OUTPUT_DIM]) {
        // Hidden layer: tanh(W1 * x + b1)
        let mut hidden = [0.0f32; HIDDEN_DIM];
        for i in 0..HIDDEN_DIM {
            let mut sum = self.b1[i];
            for j in 0..INPUT_DIM {
                sum += self.w1[i][j] * input[j];
            }
            hidden[i] = sum.tanh();
        }

        // Output layer: W2 * hidden + b2
        let mut output = [0.0f32; OUTPUT_DIM];
        for i in 0..OUTPUT_DIM {
            let mut sum = self.b2[i];
            for j in 0..HIDDEN_DIM {
                sum += self.w2[i][j] * hidden[j];
            }
            output[i] = sum;
        }

        (hidden, output)
    }

    /// Predict a ModulationVector from state features.
    pub fn predict(&self, features: &StateFeatures) -> ModulationVector {
        let input = features.as_array();
        let (_, raw) = self.forward_raw(&input);

        // Map raw outputs to valid ModulationVector ranges
        ModulationVector {
            max_tokens_factor: sigmoid(raw[0]) * 1.2 + 0.3,       // 0.3 - 1.5
            temperature_delta: raw[1].tanh() * 0.4,                // -0.4 to 0.4
            context_budget_factor: sigmoid(raw[2]) * 0.8 + 0.4,   // 0.4 - 1.2
            recall_mood_bias: raw[3].tanh(),                       // -1.0 to 1.0
            silence_inclination: sigmoid(raw[4]),                  // 0.0 - 1.0
            typing_speed_factor: sigmoid(raw[5]) * 1.5 + 0.5,     // 0.5 - 2.0
        }
    }

    /// Blend neural prediction with curves-based ModulationVector.
    pub fn blend_with(&self, curves_mv: &ModulationVector, features: &StateFeatures) -> ModulationVector {
        if self.blend <= 0.0 {
            return curves_mv.clone();
        }
        let neural_mv = self.predict(features);
        if self.blend >= 1.0 {
            return neural_mv;
        }
        curves_mv.lerp(&neural_mv, self.blend)
    }

    /// Train on a batch of samples using reward-weighted gradient descent.
    /// Each sample: (features, target_modulation, reward).
    /// Positive reward → nudge toward that modulation; negative → nudge away.
    pub fn train(&mut self, samples: &[(StateFeatures, ModulationVector, f32)], lr: f32) {
        if samples.is_empty() {
            return;
        }

        for (features, target, reward) in samples {
            if reward.abs() < 0.1 {
                continue; // Skip near-zero reward
            }

            let input = features.as_array();
            let (hidden, raw_output) = self.forward_raw(&input);
            let predicted = self.predict(features);
            let target_raw = modulation_to_raw(target);

            // Compute error: (predicted_raw - target_raw) * sign(reward)
            // Positive reward → minimize distance to target
            // Negative reward → maximize distance from target
            let sign = if *reward > 0.0 { 1.0 } else { -1.0 };
            let scale = lr * reward.abs() * sign;

            // Output layer gradients
            let mut d_output = [0.0f32; OUTPUT_DIM];
            for i in 0..OUTPUT_DIM {
                d_output[i] = (target_raw[i] - raw_output[i]) * scale;
            }

            // Update W2, b2
            for i in 0..OUTPUT_DIM {
                for j in 0..HIDDEN_DIM {
                    self.w2[i][j] += d_output[i] * hidden[j];
                }
                self.b2[i] += d_output[i];
            }

            // Backprop to hidden layer
            let mut d_hidden = [0.0f32; HIDDEN_DIM];
            for j in 0..HIDDEN_DIM {
                let mut sum = 0.0;
                for i in 0..OUTPUT_DIM {
                    sum += d_output[i] * self.w2[i][j];
                }
                // tanh derivative: 1 - tanh^2
                d_hidden[j] = sum * (1.0 - hidden[j] * hidden[j]);
            }

            // Update W1, b1
            for i in 0..HIDDEN_DIM {
                for j in 0..INPUT_DIM {
                    self.w1[i][j] += d_hidden[i] * input[j];
                }
                self.b1[i] += d_hidden[i];
            }

            // Clamp weights to prevent explosion
            clamp_weights(&mut self.w1, 5.0);
            clamp_weights(&mut self.w2, 5.0);

            // After training, if we have enough samples, increase blend
            let _ = predicted; // used above via predict()
        }
    }

    /// Serialize to JSON for persistence.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Deserialize from JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl Default for NeuralModulator {
    fn default() -> Self {
        Self::new()
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// Inverse of the output activation to get raw target values for training.
fn modulation_to_raw(mv: &ModulationVector) -> [f32; OUTPUT_DIM] {
    [
        inv_sigmoid((mv.max_tokens_factor - 0.3) / 1.2),
        mv.temperature_delta.atanh().clamp(-3.0, 3.0),
        inv_sigmoid((mv.context_budget_factor - 0.4) / 0.8),
        mv.recall_mood_bias.atanh().clamp(-3.0, 3.0),
        inv_sigmoid(mv.silence_inclination),
        inv_sigmoid((mv.typing_speed_factor - 0.5) / 1.5),
    ]
}

fn inv_sigmoid(y: f32) -> f32 {
    let y = y.clamp(0.01, 0.99);
    (y / (1.0 - y)).ln()
}

fn clamp_weights(weights: &mut [Vec<f32>], max_abs: f32) {
    for row in weights.iter_mut() {
        for w in row.iter_mut() {
            *w = w.clamp(-max_abs, max_abs);
        }
    }
}

// ============================================================================
// ADR-016: Liquid Time-Constant Neural Modulator
// ============================================================================

/// Liquid Time-Constant (LTC) neural modulator (ADR-016).
///
/// Core ODE per hidden neuron i:
///   dx_i/dt = -(1/τ_i + f_i) · x_i + A · f_i
/// where f_i = σ(W_in · input + W_rec · x + b)_i
///
/// When input is intense, f spikes → effective τ shrinks → subjective time accelerates.
/// When idle, f ≈ 0 → state decays slowly at base rate 1/τ.
#[derive(Clone, Serialize, Deserialize)]
pub struct LiquidNeuralModulator {
    /// Input → hidden weights (HIDDEN_DIM × INPUT_DIM)
    w_in: Vec<Vec<f32>>,
    /// Recurrent weights (HIDDEN_DIM × HIDDEN_DIM)
    w_rec: Vec<Vec<f32>>,
    /// Hidden biases
    b_h: Vec<f32>,
    /// Hidden → output weights (OUTPUT_DIM × HIDDEN_DIM)
    w_out: Vec<Vec<f32>>,
    /// Output biases
    b_out: Vec<f32>,
    /// Base time constants per neuron (seconds). Higher = slower decay.
    tau: Vec<f32>,
    /// Synaptic drive amplitude (A in the ODE)
    amplitude: f32,
    /// Persistent hidden state
    pub state: Vec<f32>,
    /// Blend factor: 0.0 = pure curves, 1.0 = pure LTC
    pub blend: f32,
}

impl LiquidNeuralModulator {
    pub fn new() -> Self {
        let mut seed: u64 = 0xDEAD_BEEF;
        let mut rng = move || -> f32 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            ((seed as f32) / (u64::MAX as f32)) * 2.0 - 1.0
        };

        let scale_in = (2.0f32 / INPUT_DIM as f32).sqrt();
        let scale_rec = (2.0f32 / HIDDEN_DIM as f32).sqrt() * 0.5; // smaller recurrent init

        let w_in = (0..HIDDEN_DIM)
            .map(|_| (0..INPUT_DIM).map(|_| rng() * scale_in).collect())
            .collect();
        let w_rec = (0..HIDDEN_DIM)
            .map(|_| (0..HIDDEN_DIM).map(|_| rng() * scale_rec).collect())
            .collect();
        let b_h = vec![0.0; HIDDEN_DIM];
        let w_out = (0..OUTPUT_DIM)
            .map(|_| (0..HIDDEN_DIM).map(|_| rng() * scale_in).collect())
            .collect();
        let b_out = vec![0.0; OUTPUT_DIM];

        // Base τ: 5–15 seconds (diverse time scales within the network)
        let tau = (0..HIDDEN_DIM)
            .map(|i| 5.0 + 10.0 * (i as f32 / (HIDDEN_DIM - 1) as f32))
            .collect();

        Self {
            w_in,
            w_rec,
            b_h,
            w_out,
            b_out,
            tau,
            amplitude: 1.0,
            state: vec![0.0; HIDDEN_DIM],
            blend: 0.0,
        }
    }

    /// Advance the LTC hidden state by dt seconds given input features.
    /// Returns the resulting ModulationVector.
    pub fn step(&mut self, features: &StateFeatures, dt_secs: f32) -> ModulationVector {
        let input = features.as_array();
        let dt = dt_secs.min(30.0); // cap to prevent explosion on large gaps

        // Compute synaptic activation: f = σ(W_in · input + W_rec · x + b)
        let mut f = [0.0f32; HIDDEN_DIM];
        for i in 0..HIDDEN_DIM {
            let mut sum = self.b_h[i];
            for j in 0..INPUT_DIM {
                sum += self.w_in[i][j] * input[j];
            }
            for j in 0..HIDDEN_DIM {
                sum += self.w_rec[i][j] * self.state[j];
            }
            f[i] = sigmoid(sum);
        }

        // LTC ODE step (Euler): dx_i = [-(1/τ_i + f_i)·x_i + A·f_i] · dt
        for i in 0..HIDDEN_DIM {
            let leak = 1.0 / self.tau[i];
            let dx = -(leak + f[i]) * self.state[i] + self.amplitude * f[i];
            self.state[i] += dx * dt;
            self.state[i] = self.state[i].clamp(-5.0, 5.0);
        }

        // Read out: W_out · state + b_out → ModulationVector
        self.readout()
    }

    /// Read the current hidden state into a ModulationVector (no state mutation).
    fn readout(&self) -> ModulationVector {
        let mut raw = [0.0f32; OUTPUT_DIM];
        for i in 0..OUTPUT_DIM {
            let mut sum = self.b_out[i];
            for j in 0..HIDDEN_DIM {
                sum += self.w_out[i][j] * self.state[j];
            }
            raw[i] = sum;
        }

        ModulationVector {
            max_tokens_factor: sigmoid(raw[0]) * 1.2 + 0.3,
            temperature_delta: raw[1].tanh() * 0.4,
            context_budget_factor: sigmoid(raw[2]) * 0.8 + 0.4,
            recall_mood_bias: raw[3].tanh(),
            silence_inclination: sigmoid(raw[4]),
            typing_speed_factor: sigmoid(raw[5]) * 1.5 + 0.5,
        }
    }

    /// Blend LTC output with curves-based ModulationVector.
    pub fn blend_with(&mut self, curves_mv: &ModulationVector, features: &StateFeatures, dt_secs: f32) -> ModulationVector {
        if self.blend <= 0.0 {
            return curves_mv.clone();
        }
        let ltc_mv = self.step(features, dt_secs);
        if self.blend >= 1.0 {
            return ltc_mv;
        }
        curves_mv.lerp(&ltc_mv, self.blend)
    }

    /// ADR-017: Hebbian weight update modulated by surprise/reward.
    ///
    /// ΔW_ij = η · S · (x_i · x_j) − λ · W_ij
    pub fn hebbian_update(&mut self, surprise: f32, reward: f32, lr: f32) {
        let s = (surprise + reward.abs()) * 0.5; // combined modulation signal
        if s < 0.05 {
            return; // skip trivial updates
        }
        let lambda = 0.001; // forgetting factor

        // Update recurrent weights (Hebbian: cells that fire together wire together)
        for i in 0..HIDDEN_DIM {
            for j in 0..HIDDEN_DIM {
                let dw = lr * s * (self.state[i] * self.state[j]) - lambda * self.w_rec[i][j];
                self.w_rec[i][j] += dw;
                self.w_rec[i][j] = self.w_rec[i][j].clamp(-5.0, 5.0);
            }
        }
    }

    /// Effective time constant for neuron i (diagnostic).
    /// When f is high, effective τ shrinks → faster dynamics.
    pub fn effective_tau(&self, neuron: usize, f_activation: f32) -> f32 {
        1.0 / (1.0 / self.tau[neuron] + f_activation)
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

impl Default for LiquidNeuralModulator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_predict_valid_ranges() {
        let nn = NeuralModulator::new();
        let features = StateFeatures {
            energy: 0.5, stress: 0.3, arousal: 0.4, mood_bias: 0.0, social_need: 0.2,
        };
        let mv = nn.predict(&features);
        assert!(mv.max_tokens_factor >= 0.3 && mv.max_tokens_factor <= 1.5);
        assert!(mv.temperature_delta >= -0.4 && mv.temperature_delta <= 0.4);
        assert!(mv.context_budget_factor >= 0.4 && mv.context_budget_factor <= 1.2);
        assert!(mv.recall_mood_bias >= -1.0 && mv.recall_mood_bias <= 1.0);
        assert!(mv.silence_inclination >= 0.0 && mv.silence_inclination <= 1.0);
        assert!(mv.typing_speed_factor >= 0.5 && mv.typing_speed_factor <= 2.0);
    }

    #[test]
    fn test_blend_zero_returns_curves() {
        let nn = NeuralModulator::new(); // blend = 0.0
        let curves_mv = ModulationVector::default();
        let features = StateFeatures {
            energy: 0.7, stress: 0.1, arousal: 0.3, mood_bias: 0.2, social_need: 0.1,
        };
        let result = nn.blend_with(&curves_mv, &features);
        assert!((result.max_tokens_factor - curves_mv.max_tokens_factor).abs() < 1e-6);
    }

    #[test]
    fn test_train_moves_toward_target() {
        let mut nn = NeuralModulator::new();
        nn.blend = 1.0;
        let features = StateFeatures {
            energy: 0.8, stress: 0.1, arousal: 0.5, mood_bias: 0.3, social_need: 0.2,
        };
        let target = ModulationVector {
            max_tokens_factor: 1.2,
            temperature_delta: 0.1,
            context_budget_factor: 1.0,
            recall_mood_bias: 0.3,
            silence_inclination: 0.1,
            typing_speed_factor: 1.5,
        };

        let before = nn.predict(&features);
        let dist_before = (before.max_tokens_factor - target.max_tokens_factor).abs();

        // Train with positive reward
        let samples = vec![(features.clone(), target.clone(), 0.8)];
        for _ in 0..50 {
            nn.train(&samples, 0.01);
        }

        let after = nn.predict(&features);
        let dist_after = (after.max_tokens_factor - target.max_tokens_factor).abs();
        assert!(dist_after < dist_before, "Training should move prediction toward target");
    }

    #[test]
    fn test_serialization_roundtrip() {
        let nn = NeuralModulator::new();
        let json = nn.to_json().unwrap();
        let restored = NeuralModulator::from_json(&json).unwrap();
        assert_eq!(nn.w1.len(), restored.w1.len());
        assert!((nn.blend - restored.blend).abs() < 1e-6);
    }

    // === LTC tests ===

    #[test]
    fn test_ltc_step_valid_ranges() {
        let mut ltc = LiquidNeuralModulator::new();
        ltc.blend = 1.0;
        let features = StateFeatures {
            energy: 0.5, stress: 0.3, arousal: 0.4, mood_bias: 0.0, social_need: 0.2,
        };
        let mv = ltc.step(&features, 1.0);
        assert!(mv.max_tokens_factor >= 0.3 && mv.max_tokens_factor <= 1.5);
        assert!(mv.temperature_delta >= -0.4 && mv.temperature_delta <= 0.4);
        assert!(mv.context_budget_factor >= 0.4 && mv.context_budget_factor <= 1.2);
        assert!(mv.silence_inclination >= 0.0 && mv.silence_inclination <= 1.0);
    }

    #[test]
    fn test_ltc_state_persistence() {
        let mut ltc = LiquidNeuralModulator::new();
        let features = StateFeatures {
            energy: 0.8, stress: 0.1, arousal: 0.7, mood_bias: 0.5, social_need: 0.1,
        };
        // Step once — state should change from zero
        ltc.step(&features, 1.0);
        let state_after_one = ltc.state.clone();
        assert!(state_after_one.iter().any(|&x| x.abs() > 1e-6),
            "Hidden state should be non-zero after a step");

        // Step again — state should differ (it's a dynamical system)
        ltc.step(&features, 1.0);
        assert_ne!(ltc.state, state_after_one,
            "Hidden state should evolve between steps");
    }

    #[test]
    fn test_ltc_idle_decay() {
        let mut ltc = LiquidNeuralModulator::new();
        let active = StateFeatures {
            energy: 0.8, stress: 0.1, arousal: 0.9, mood_bias: 0.5, social_need: 0.1,
        };
        // Drive state with active input
        for _ in 0..20 {
            ltc.step(&active, 1.0);
        }
        let active_norm: f32 = ltc.state.iter().map(|x| x * x).sum();

        // Now idle — state should decay toward zero
        let idle = StateFeatures {
            energy: 0.7, stress: 0.0, arousal: 0.0, mood_bias: 0.0, social_need: 0.0,
        };
        for _ in 0..100 {
            ltc.step(&idle, 1.0);
        }
        let idle_norm: f32 = ltc.state.iter().map(|x| x * x).sum();
        assert!(idle_norm < active_norm, "State should decay during idle");
    }

    #[test]
    fn test_ltc_hebbian_modifies_weights() {
        let mut ltc = LiquidNeuralModulator::new();
        let features = StateFeatures {
            energy: 0.5, stress: 0.5, arousal: 0.5, mood_bias: 0.0, social_need: 0.5,
        };
        // Drive state so Hebbian has something to work with
        for _ in 0..10 {
            ltc.step(&features, 1.0);
        }
        let w_before = ltc.w_rec.clone();
        ltc.hebbian_update(0.8, 0.5, 0.01);
        assert_ne!(ltc.w_rec, w_before, "Hebbian update should modify recurrent weights");
    }

    #[test]
    fn test_ltc_effective_tau_shrinks_with_activation() {
        let ltc = LiquidNeuralModulator::new();
        let base = ltc.effective_tau(0, 0.0);
        let active = ltc.effective_tau(0, 0.5);
        assert!(active < base, "Effective τ should shrink with activation: {} < {}", active, base);
    }

    #[test]
    fn test_ltc_serialization_roundtrip() {
        let mut ltc = LiquidNeuralModulator::new();
        ltc.state[0] = 0.42;
        ltc.blend = 0.7;
        let json = ltc.to_json().unwrap();
        let restored = LiquidNeuralModulator::from_json(&json).unwrap();
        assert!((restored.state[0] - 0.42).abs() < 1e-6);
        assert!((restored.blend - 0.7).abs() < 1e-6);
    }
}
