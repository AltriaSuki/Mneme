//! Property-based tests for mneme_limbic somatic markers and modulation vector.
//!
//! Verifies that the ModulationVector output always stays within documented
//! bounds regardless of input OrganismState, and that SomaticMarker methods
//! never panic on arbitrary state.

use mneme_core::state::AttachmentState;
use mneme_core::{Affect, FastState, MediumState, OrganismState, SlowState};
use mneme_limbic::{ModulationVector, SomaticMarker};
use proptest::prelude::*;

// ============================================================================
// Strategies
// ============================================================================

fn arb_affect() -> impl Strategy<Value = Affect> {
    (-1.0f32..=1.0, 0.0f32..=1.0).prop_map(|(v, a)| Affect::new(v, a))
}

fn arb_fast_state() -> impl Strategy<Value = FastState> {
    (
        arb_affect(),
        0.0f32..=1.0,
        0.0f32..=1.0,
        0.0f32..=1.0,
        0.0f32..=1.0,
        0.0f32..=1.0,
    )
        .prop_map(
            |(affect, energy, stress, curiosity, social_need, boredom)| FastState {
                affect,
                energy,
                stress,
                curiosity,
                social_need,
                boredom,
                curiosity_vector: Default::default(),
            },
        )
}

fn arb_medium_state() -> impl Strategy<Value = MediumState> {
    (
        -1.0f32..=1.0,
        0.0f32..=1.0,
        0.0f32..=1.0,
        0.0f32..=1.0,
        0.0f32..=1.0,
    )
        .prop_map(
            |(mood_bias, anxiety, avoidance, openness, hunger)| MediumState {
                mood_bias,
                attachment: AttachmentState { anxiety, avoidance },
                openness,
                hunger,
            },
        )
}

fn arb_organism_state() -> impl Strategy<Value = OrganismState> {
    (arb_fast_state(), arb_medium_state()).prop_map(|(fast, medium)| OrganismState {
        fast,
        medium,
        slow: SlowState::default(),
        last_updated: 0,
    })
}

// ============================================================================
// ModulationVector Bound Properties
// ============================================================================

proptest! {
    /// **Core invariant**: to_modulation_vector() output is ALWAYS within
    /// the documented ranges, for any valid OrganismState.
    ///
    /// Documented ranges (from somatic.rs):
    ///   max_tokens_factor:    [0.3, 1.5]
    ///   temperature_delta:    [-0.3, 0.4]  (code clamps to [-0.1, 0.4])
    ///   context_budget_factor:[0.4, 1.2]
    ///   recall_mood_bias:     [-1.0, 1.0]
    ///   silence_inclination:  [0.0, 1.0]
    ///   typing_speed_factor:  [0.5, 2.0]
    #[test]
    fn modulation_vector_always_in_bounds(state in arb_organism_state()) {
        let marker = SomaticMarker::from_state(&state);
        let mv = marker.to_modulation_vector();

        // max_tokens_factor: lerp(0.3, 1.2, energy) → [0.3, 1.2]
        prop_assert!(mv.max_tokens_factor >= 0.3 && mv.max_tokens_factor <= 1.5,
            "max_tokens_factor out of range: {} (energy={})", mv.max_tokens_factor, state.fast.energy);

        // temperature_delta: clamped to [-0.1, 0.4]
        prop_assert!(mv.temperature_delta >= -0.3 && mv.temperature_delta <= 0.4,
            "temperature_delta out of range: {} (stress={}, arousal={})",
            mv.temperature_delta, state.fast.stress, state.fast.affect.arousal);

        // context_budget_factor: clamped to [0.4, 1.2]
        prop_assert!(mv.context_budget_factor >= 0.4 && mv.context_budget_factor <= 1.2,
            "context_budget_factor out of range: {} (energy={}, stress={})",
            mv.context_budget_factor, state.fast.energy, state.fast.stress);

        // recall_mood_bias: clamped to [-1.0, 1.0]
        prop_assert!(mv.recall_mood_bias >= -1.0 && mv.recall_mood_bias <= 1.0,
            "recall_mood_bias out of range: {}", mv.recall_mood_bias);

        // silence_inclination: clamped to [0.0, 1.0]
        prop_assert!(mv.silence_inclination >= 0.0 && mv.silence_inclination <= 1.0,
            "silence_inclination out of range: {} (energy={}, social_need={}, stress={})",
            mv.silence_inclination, state.fast.energy, state.fast.social_need, state.fast.stress);

        // typing_speed_factor: lerp(0.6, 1.8, arousal) → [0.6, 1.8] ⊂ [0.5, 2.0]
        prop_assert!(mv.typing_speed_factor >= 0.5 && mv.typing_speed_factor <= 2.0,
            "typing_speed_factor out of range: {} (arousal={})",
            mv.typing_speed_factor, state.fast.affect.arousal);

        // All fields must be finite
        prop_assert!(mv.max_tokens_factor.is_finite());
        prop_assert!(mv.temperature_delta.is_finite());
        prop_assert!(mv.context_budget_factor.is_finite());
        prop_assert!(mv.recall_mood_bias.is_finite());
        prop_assert!(mv.silence_inclination.is_finite());
        prop_assert!(mv.typing_speed_factor.is_finite());
    }

    /// **Monotonicity**: higher energy → higher max_tokens_factor (all else equal).
    #[test]
    fn modulation_max_tokens_monotonic_in_energy(
        state in arb_organism_state(),
        e1 in 0.0f32..=0.49,
        e2 in 0.51f32..=1.0,
    ) {
        let mut s1 = state.clone();
        s1.fast.energy = e1;
        let mv1 = SomaticMarker::from_state(&s1).to_modulation_vector();

        let mut s2 = state;
        s2.fast.energy = e2;
        let mv2 = SomaticMarker::from_state(&s2).to_modulation_vector();

        prop_assert!(mv2.max_tokens_factor >= mv1.max_tokens_factor,
            "energy {} → mtf {}, energy {} → mtf {} (not monotonic)",
            e1, mv1.max_tokens_factor, e2, mv2.max_tokens_factor);
    }

    /// **Monotonicity**: higher stress → higher temperature_delta (all else equal).
    #[test]
    fn modulation_temperature_monotonic_in_stress(
        state in arb_organism_state(),
        s1 in 0.0f32..=0.3,
        s2 in 0.7f32..=1.0,
    ) {
        let mut lo = state.clone();
        lo.fast.stress = s1;
        let mv_lo = SomaticMarker::from_state(&lo).to_modulation_vector();

        let mut hi = state;
        hi.fast.stress = s2;
        let mv_hi = SomaticMarker::from_state(&hi).to_modulation_vector();

        prop_assert!(mv_hi.temperature_delta >= mv_lo.temperature_delta,
            "stress {} → temp {}, stress {} → temp {} (not monotonic)",
            s1, mv_lo.temperature_delta, s2, mv_hi.temperature_delta);
    }
}

// ============================================================================
// SomaticMarker Properties
// ============================================================================

proptest! {
    /// **SomaticMarker::from_state never panics** for any valid state.
    #[test]
    fn somatic_marker_from_state_never_panics(state in arb_organism_state()) {
        let marker = SomaticMarker::from_state(&state);
        // Marker should have valid numeric fields
        prop_assert!(marker.energy >= 0.0 && marker.energy <= 1.0);
    }

    /// **format_for_prompt** always produces a well-formed string.
    #[test]
    fn format_for_prompt_well_formed(state in arb_organism_state()) {
        let marker = SomaticMarker::from_state(&state);
        let prompt = marker.format_for_prompt();
        prop_assert!(prompt.starts_with("[内部状态:"));
        prop_assert!(prompt.ends_with("]"));
        prop_assert!(prompt.contains("E="));
    }

    /// **proactivity_urgency** is always in [0, 1].
    #[test]
    fn proactivity_urgency_bounded(state in arb_organism_state()) {
        let marker = SomaticMarker::from_state(&state);
        let u = marker.proactivity_urgency();
        prop_assert!(u >= 0.0 && u <= 1.0, "urgency out of range: {}", u);
        prop_assert!(u.is_finite());
    }

    /// **needs_attention** never panics.
    #[test]
    fn needs_attention_never_panics(state in arb_organism_state()) {
        let marker = SomaticMarker::from_state(&state);
        let _ = marker.needs_attention();
    }
}

// ============================================================================
// ModulationVector Default
// ============================================================================

#[test]
fn modulation_vector_default_is_neutral() {
    let mv = ModulationVector::default();
    assert_eq!(mv.max_tokens_factor, 1.0);
    assert_eq!(mv.temperature_delta, 0.0);
    assert_eq!(mv.context_budget_factor, 1.0);
    assert_eq!(mv.recall_mood_bias, 0.0);
    assert_eq!(mv.silence_inclination, 0.0);
    assert_eq!(mv.typing_speed_factor, 1.0);
}
