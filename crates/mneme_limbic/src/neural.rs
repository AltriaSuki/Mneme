//! #14: Small MLP neural network that learns state → ModulationVector mapping.
//!
//! Architecture: 5 inputs → 8 hidden (tanh) → 6 outputs
//! Replaces/blends with ModulationCurves (Layer 1) as Layer 2.

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
}
