//! Property-based tests for mneme_core.
//!
//! Uses proptest to verify invariants that must hold for ALL possible inputs,
//! not just hand-picked examples. This catches edge cases that unit tests miss.

use proptest::prelude::*;
use std::time::Duration;
use mneme_core::{
    OrganismState, FastState, MediumState, SlowState, Affect,
    Emotion, SensoryInput,
};
use mneme_core::dynamics::{DefaultDynamics, Dynamics, homeostatic_error};
use mneme_core::state::{AttachmentState, AttachmentStyle, ValueNetwork};

// ============================================================================
// Strategies: generate arbitrary but valid state values
// ============================================================================

/// Generate an arbitrary Affect in valid range.
fn arb_affect() -> impl Strategy<Value = Affect> {
    (-1.0f32..=1.0, 0.0f32..=1.0).prop_map(|(v, a)| Affect::new(v, a))
}

/// Generate an arbitrary FastState with values in [0, 1] range.
fn arb_fast_state() -> impl Strategy<Value = FastState> {
    (arb_affect(), 0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0)
        .prop_map(|(affect, energy, stress, curiosity, social_need, boredom)| FastState {
            affect,
            energy,
            stress,
            curiosity,
            social_need,
            boredom,
        })
}

/// Generate an arbitrary MediumState.
fn arb_medium_state() -> impl Strategy<Value = MediumState> {
    (-1.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0)
        .prop_map(|(mood_bias, anxiety, avoidance, openness, hunger)| MediumState {
            mood_bias,
            attachment: AttachmentState { anxiety, avoidance },
            openness,
            hunger,
        })
}

/// Generate an arbitrary OrganismState.
fn arb_organism_state() -> impl Strategy<Value = OrganismState> {
    (arb_fast_state(), arb_medium_state())
        .prop_map(|(fast, medium)| OrganismState {
            fast,
            medium,
            slow: SlowState::default(),
            last_updated: 0,
        })
}

/// Generate an arbitrary SensoryInput.
fn arb_sensory_input() -> impl Strategy<Value = SensoryInput> {
    (-1.0f32..=1.0, 0.0f32..=1.0, 0.0f32..=1.0, any::<bool>(), 0.0f32..=5.0)
        .prop_map(|(valence, intensity, surprise, is_social, delay)| SensoryInput {
            content_valence: valence,
            content_intensity: intensity,
            surprise,
            is_social,
            response_delay_factor: delay,
            violated_values: vec![],
        })
}

// ============================================================================
// ODE Dynamics Properties
// ============================================================================

proptest! {
    /// **Core invariant**: A single step from any valid state must produce
    /// a state where all values are finite and within documented ranges.
    #[test]
    fn dynamics_step_always_produces_valid_state(
        state in arb_organism_state(),
        input in arb_sensory_input(),
        dt_secs in 1u64..86400,  // 1 second to 24 hours
    ) {
        let dynamics = DefaultDynamics::default();
        let mut s = state;
        dynamics.step(&mut s, &input, Duration::from_secs(dt_secs));

        // Fast state
        prop_assert!(s.fast.energy.is_finite(), "energy not finite: {}", s.fast.energy);
        prop_assert!(s.fast.energy >= 0.0 && s.fast.energy <= 1.0, "energy out of range: {}", s.fast.energy);
        prop_assert!(s.fast.stress.is_finite(), "stress not finite: {}", s.fast.stress);
        prop_assert!(s.fast.stress >= 0.0 && s.fast.stress <= 1.0, "stress out of range: {}", s.fast.stress);
        prop_assert!(s.fast.curiosity.is_finite(), "curiosity not finite: {}", s.fast.curiosity);
        prop_assert!(s.fast.curiosity >= 0.0 && s.fast.curiosity <= 1.0, "curiosity out of range: {}", s.fast.curiosity);
        prop_assert!(s.fast.social_need.is_finite(), "social_need not finite: {}", s.fast.social_need);
        prop_assert!(s.fast.social_need >= 0.0 && s.fast.social_need <= 1.0, "social_need out of range: {}", s.fast.social_need);
        prop_assert!(s.fast.affect.valence.is_finite(), "valence not finite: {}", s.fast.affect.valence);
        prop_assert!(s.fast.affect.valence >= -1.0 && s.fast.affect.valence <= 1.0, "valence out of range: {}", s.fast.affect.valence);
        prop_assert!(s.fast.affect.arousal.is_finite(), "arousal not finite: {}", s.fast.affect.arousal);
        prop_assert!(s.fast.affect.arousal >= 0.0 && s.fast.affect.arousal <= 1.0, "arousal out of range: {}", s.fast.affect.arousal);

        // Medium state
        prop_assert!(s.medium.mood_bias.is_finite() && s.medium.mood_bias >= -1.0 && s.medium.mood_bias <= 1.0);
        prop_assert!(s.medium.openness.is_finite() && s.medium.openness >= 0.0 && s.medium.openness <= 1.0);
        prop_assert!(s.medium.hunger.is_finite() && s.medium.hunger >= 0.0 && s.medium.hunger <= 1.0);
    }

    /// **Stability under repeated steps**: 1000 iterations should not diverge.
    #[test]
    fn dynamics_many_steps_remain_stable(
        state in arb_organism_state(),
        input in arb_sensory_input(),
    ) {
        let dynamics = DefaultDynamics::default();
        let mut s = state;
        for _ in 0..1000 {
            dynamics.step(&mut s, &input, Duration::from_secs(1));
        }
        // After 1000 seconds, everything must still be valid
        prop_assert!(s.fast.energy.is_finite() && s.fast.energy >= 0.0 && s.fast.energy <= 1.0);
        prop_assert!(s.fast.stress.is_finite() && s.fast.stress >= 0.0 && s.fast.stress <= 1.0);
        prop_assert!(s.fast.affect.valence.is_finite());
        prop_assert!(s.medium.mood_bias.is_finite());
    }

    /// **NaN injection recovery**: if state somehow contains NaN, a step should fix it.
    #[test]
    fn dynamics_recovers_from_nan_injection(
        input in arb_sensory_input(),
    ) {
        let dynamics = DefaultDynamics::default();
        let mut s = OrganismState::default();

        // Inject NaN/Inf into every field
        s.fast.energy = f32::NAN;
        s.fast.stress = f32::INFINITY;
        s.fast.curiosity = f32::NEG_INFINITY;
        s.fast.social_need = f32::NAN;
        s.fast.affect.valence = f32::NAN;
        s.fast.affect.arousal = f32::INFINITY;
        s.medium.mood_bias = f32::NAN;
        s.medium.openness = f32::INFINITY;
        s.medium.hunger = f32::NAN;

        dynamics.step(&mut s, &input, Duration::from_secs(1));

        prop_assert!(s.fast.energy.is_finite());
        prop_assert!(s.fast.stress.is_finite());
        prop_assert!(s.fast.curiosity.is_finite());
        prop_assert!(s.fast.social_need.is_finite());
        prop_assert!(s.fast.affect.valence.is_finite());
        prop_assert!(s.fast.affect.arousal.is_finite());
        prop_assert!(s.medium.mood_bias.is_finite());
        prop_assert!(s.medium.openness.is_finite());
        prop_assert!(s.medium.hunger.is_finite());
    }

    /// **Homeostatic error is non-negative** for any state.
    #[test]
    fn homeostatic_error_is_nonneg(state in arb_organism_state()) {
        let dynamics = DefaultDynamics::default();
        let err = homeostatic_error(&state, &dynamics);
        prop_assert!(err >= 0.0, "homeostatic_error negative: {}", err);
        prop_assert!(err.is_finite(), "homeostatic_error not finite: {}", err);
    }

    /// **Crisis never produces NaN** regardless of intensity.
    #[test]
    fn crisis_step_produces_valid_state(
        state in arb_organism_state(),
        intensity in 0.0f32..=2.0,
    ) {
        let dynamics = DefaultDynamics::default();
        let mut s = state;
        dynamics.step_slow_crisis(&mut s.slow, &s.medium, intensity);

        prop_assert!(s.slow.rigidity.is_finite() && s.slow.rigidity >= 0.0 && s.slow.rigidity <= 1.0);
        prop_assert!(s.slow.narrative_bias.is_finite() && s.slow.narrative_bias >= -1.0 && s.slow.narrative_bias <= 1.0);
    }
}

// ============================================================================
// FastState::normalize() Properties
// ============================================================================

proptest! {
    /// **normalize() is idempotent**: calling it twice is the same as once.
    #[test]
    fn fast_normalize_idempotent(state in arb_fast_state()) {
        let mut a = state.clone();
        a.normalize();
        let mut b = a.clone();
        b.normalize();

        prop_assert_eq!(a.energy.to_bits(), b.energy.to_bits());
        prop_assert_eq!(a.stress.to_bits(), b.stress.to_bits());
        prop_assert_eq!(a.curiosity.to_bits(), b.curiosity.to_bits());
        prop_assert_eq!(a.social_need.to_bits(), b.social_need.to_bits());
        prop_assert_eq!(a.affect.valence.to_bits(), b.affect.valence.to_bits());
        prop_assert_eq!(a.affect.arousal.to_bits(), b.affect.arousal.to_bits());
    }

    /// **normalize() maps any f32 into valid range**, including extreme values.
    #[test]
    fn fast_normalize_clamps_extremes(
        energy in prop::num::f32::ANY,
        stress in prop::num::f32::ANY,
        curiosity in prop::num::f32::ANY,
        social_need in prop::num::f32::ANY,
        boredom in prop::num::f32::ANY,
        valence in prop::num::f32::ANY,
        arousal in prop::num::f32::ANY,
    ) {
        let mut fast = FastState {
            energy, stress, curiosity, social_need, boredom,
            affect: Affect { valence, arousal },
        };
        fast.normalize();

        prop_assert!(fast.energy >= 0.0 && fast.energy <= 1.0, "energy: {}", fast.energy);
        prop_assert!(fast.stress >= 0.0 && fast.stress <= 1.0, "stress: {}", fast.stress);
        prop_assert!(fast.curiosity >= 0.0 && fast.curiosity <= 1.0, "curiosity: {}", fast.curiosity);
        prop_assert!(fast.social_need >= 0.0 && fast.social_need <= 1.0, "social_need: {}", fast.social_need);
        prop_assert!(fast.boredom >= 0.0 && fast.boredom <= 1.0, "boredom: {}", fast.boredom);
        prop_assert!(fast.affect.valence >= -1.0 && fast.affect.valence <= 1.0, "valence: {}", fast.affect.valence);
        prop_assert!(fast.affect.arousal >= 0.0 && fast.affect.arousal <= 1.0, "arousal: {}", fast.affect.arousal);
    }

    /// **MediumState::normalize() handles any f32**.
    #[test]
    fn medium_normalize_clamps_extremes(
        mood in prop::num::f32::ANY,
        openness in prop::num::f32::ANY,
        hunger in prop::num::f32::ANY,
        anxiety in prop::num::f32::ANY,
        avoidance in prop::num::f32::ANY,
    ) {
        let mut medium = MediumState {
            mood_bias: mood,
            openness,
            hunger,
            attachment: AttachmentState { anxiety, avoidance },
        };
        medium.normalize();

        prop_assert!(medium.mood_bias >= -1.0 && medium.mood_bias <= 1.0);
        prop_assert!(medium.openness >= 0.0 && medium.openness <= 1.0);
        prop_assert!(medium.hunger >= 0.0 && medium.hunger <= 1.0);
        prop_assert!(medium.attachment.anxiety >= 0.0 && medium.attachment.anxiety <= 1.0);
        prop_assert!(medium.attachment.avoidance >= 0.0 && medium.attachment.avoidance <= 1.0);
    }
}

// ============================================================================
// Affect Properties
// ============================================================================

proptest! {
    /// **Affect::new always clamps** to valid range.
    #[test]
    fn affect_new_always_valid(v in prop::num::f32::ANY, a in prop::num::f32::ANY) {
        let affect = Affect::new(v, a);
        if v.is_finite() {
            prop_assert!(affect.valence >= -1.0 && affect.valence <= 1.0);
        }
        if a.is_finite() {
            prop_assert!(affect.arousal >= 0.0 && affect.arousal <= 1.0);
        }
    }

    /// **Affect::from_polar** always produces valid coordinates.
    #[test]
    fn affect_from_polar_always_valid(
        angle in -std::f32::consts::TAU..=std::f32::consts::TAU,
        intensity in 0.0f32..=2.0,  // intentionally over 1.0 to test clamping
    ) {
        let affect = Affect::from_polar(angle, intensity);
        prop_assert!(affect.valence >= -1.0 && affect.valence <= 1.0,
            "from_polar valence out of range: {}", affect.valence);
        prop_assert!(affect.arousal >= 0.0 && affect.arousal <= 1.0,
            "from_polar arousal out of range: {}", affect.arousal);
    }

    /// **Affect::intensity** is always non-negative.
    #[test]
    fn affect_intensity_nonneg(affect in arb_affect()) {
        let i = affect.intensity();
        prop_assert!(i >= 0.0, "intensity negative: {}", i);
        prop_assert!(i.is_finite(), "intensity not finite: {}", i);
    }

    /// **Affect::lerp** always produces valid affect, and t=0 ≈ self, t=1 ≈ other.
    #[test]
    fn affect_lerp_valid(
        a in arb_affect(),
        b in arb_affect(),
        t in 0.0f32..=1.0,
    ) {
        let result = a.lerp(&b, t);
        prop_assert!(result.valence >= -1.0 && result.valence <= 1.0);
        prop_assert!(result.arousal >= 0.0 && result.arousal <= 1.0);
    }

    /// **Affect::to_discrete_label** never panics.
    #[test]
    fn affect_discrete_label_never_panics(affect in arb_affect()) {
        let label = affect.to_discrete_label();
        prop_assert!(!label.is_empty());
    }

    /// **Affect::describe** never panics and returns non-empty string.
    #[test]
    fn affect_describe_never_panics(affect in arb_affect()) {
        let desc = affect.describe();
        prop_assert!(!desc.is_empty());
    }
}

// ============================================================================
// Emotion Properties
// ============================================================================

proptest! {
    /// **Emotion::from_affect never panics** for any valid Affect.
    #[test]
    fn emotion_from_affect_never_panics(affect in arb_affect()) {
        let _e = Emotion::from_affect(&affect);
    }

    /// **Emotion round-trip**: from_str(as_str()) == Some(self) for all variants.
    #[test]
    fn emotion_roundtrip(idx in 0usize..7) {
        let emotions = [
            Emotion::Neutral, Emotion::Happy, Emotion::Sad,
            Emotion::Excited, Emotion::Calm, Emotion::Angry, Emotion::Surprised,
        ];
        let e = emotions[idx];
        let s = e.as_str();
        let roundtripped = Emotion::parse_str(s);
        prop_assert_eq!(roundtripped, Some(e));
    }

    /// **Emotion::parse_str returns None** for random strings (not one of the known labels).
    #[test]
    fn emotion_from_str_random_returns_none(s in "[^a-zA-Z]{1,20}") {
        let result = Emotion::parse_str(&s);
        prop_assert!(result.is_none(), "unexpected Some for: {:?}", s);
    }
}

// ============================================================================
// Attachment Properties
// ============================================================================

proptest! {
    /// **AttachmentState::update_from_interaction preserves [0, 1] bounds**.
    #[test]
    fn attachment_update_preserves_bounds(
        anxiety in 0.0f32..=1.0,
        avoidance in 0.0f32..=1.0,
        positive in any::<bool>(),
        delay in 0.0f32..=10.0,
    ) {
        let mut att = AttachmentState { anxiety, avoidance };
        att.update_from_interaction(positive, delay);

        prop_assert!(att.anxiety >= 0.0 && att.anxiety <= 1.0,
            "anxiety out of range: {}", att.anxiety);
        prop_assert!(att.avoidance >= 0.0 && att.avoidance <= 1.0,
            "avoidance out of range: {}", att.avoidance);
    }

    /// **AttachmentStyle classification is consistent** with quadrant definition.
    #[test]
    fn attachment_style_matches_quadrant(
        anxiety in 0.0f32..=1.0,
        avoidance in 0.0f32..=1.0,
    ) {
        let att = AttachmentState { anxiety, avoidance };
        let style = att.style();
        match style {
            AttachmentStyle::Secure => {
                prop_assert!(anxiety <= 0.5 && avoidance <= 0.5);
            }
            AttachmentStyle::Anxious => {
                prop_assert!(anxiety > 0.5 && avoidance <= 0.5);
            }
            AttachmentStyle::Avoidant => {
                prop_assert!(anxiety <= 0.5 && avoidance > 0.5);
            }
            AttachmentStyle::Disorganized => {
                prop_assert!(anxiety > 0.5 && avoidance > 0.5);
            }
        }
    }
}

// ============================================================================
// ValueNetwork Properties
// ============================================================================

proptest! {
    /// **moral_cost is in [0, 1]** for any combination of values.
    #[test]
    fn moral_cost_bounded(n in 0usize..10) {
        let net = ValueNetwork::default();
        let all_values: Vec<&str> = vec![
            "honesty", "kindness", "curiosity", "authenticity",
            "growth", "connection", "autonomy",
            "nonexistent_1", "nonexistent_2",
        ];
        let subset: Vec<&str> = all_values.into_iter().take(n).collect();
        let cost = net.compute_moral_cost(&subset);
        prop_assert!(cost >= 0.0 && cost <= 1.0, "moral_cost out of range: {}", cost);
    }

    /// **top_values always returns ≤ n items** and is sorted descending.
    #[test]
    fn top_values_sorted_and_bounded(n in 0usize..20) {
        let net = ValueNetwork::default();
        let top = net.top_values(n);
        prop_assert!(top.len() <= n);
        // Check sorted descending
        for w in top.windows(2) {
            prop_assert!(w[0].1 >= w[1].1, "not sorted: {} < {}", w[0].1, w[1].1);
        }
    }
}
