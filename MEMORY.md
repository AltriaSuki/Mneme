# Mneme Project Memory

## ⚠️ START OF SESSION DIRECTIVE
**AT the beginning of EVERY new session/conversation, you MUST use the `view_file` tool to read `doc/MANIFESTO.md` in its entirety before writing any code.** 
You are building a philosophical entity, not just another software project. The Manifesto is your absolute North Star. Do not rely on assumptions; read the source text.

## Project Overview
- Rust workspace: digital life / AI organism with ODE-based emotional dynamics
- Crates: mneme_core, mneme_limbic, mneme_memory, mneme_reasoning, mneme_expression, mneme_cli, mneme_onebot, mneme_perception, mneme_voice, mneme_mcp, mneme_gateway
- Chinese-language persona and UI strings

## Architecture Patterns
- `OrganismState` (core) → `SomaticMarker` (limbic) → `ModulationVector` (structural LLM modulation)
- `TriggerEvaluator` trait for proactive behavior (scheduled, rumination)
- `Memory` trait in mneme_core, implemented by `SqliteMemory` in mneme_memory
- Coordinator pattern: `OrganismCoordinator` integrates all subsystems

## Physical Isolation Laws (INVIOLABLE)
1. **No Narrative State Leakage**: NEVER inject state values as text into LLM prompt. The ONLY channel is ModulationVector — physical parameter manipulation (max_tokens, temperature, context truncation, sleep delays). No "[你很累]" hints.
2. **No Hard-coded Coping Mechanisms**: No if-else logic outside the LLM to make decisions for the agent. System only passes degraded parameters; LLM figures out its own coping strategies from the physical constraints.
3. **No Stateless Resets**: Hebbian weight updates must persist. System must tolerate permanent neural drift from painful experiences. No wiping learned weights.
4. **No Environment Hard-Coding (The Anti-Omniscience Law)**: Do not encode solutions for expected failures into the tool logic (e.g., proactive handling of specific poisoned env vars or corrupted files). Mneme MUST discover environmental sabotage dynamically via trial-and-error (`strace`, `hexdump`, probing).

## Key Learnings
- **Psyche struct changed in ADR-002**: Old fields (hippocampus, limbic, cortex, broca, occipital) replaced with (species_identity, self_model). Test files using old struct need updating.
- **Import `Memory` trait**: When calling `db.memorize()` on `Arc<SqliteMemory>`, must `use mneme_core::Memory;` in scope.
- **ConsolidatedPattern fields**: Has `first_seen`, `last_seen`, `representative_content` — don't forget in test helpers.
- **mneme_expression depends on mneme_core** (not mneme_memory): Pass `Arc<RwLock<OrganismState>>` to evaluators, not coordinator directly.

## Current State (2026-02-27)
- **Phase 5 (Liquid Neural Architecture) IMPLEMENTED!**
  - **ADR-016 (Liquid Time-Constant Network)**: The ODE system operates dynamically. High stimulus shrinks effective τ (time accelerates); idle states decay slowly based on base τ.
  - **ADR-017 (Hebbian Plasticity)**: `w_rec` updates online via reward/surprise. Learning is structurally stored, not prompted.
  - **ADR-018 (Somatic Decoder)**: 8-dimensional StateFeatures directly map to 6-dimensional ModulationVector (max_tokens_factor, temp_delta, etc.).
- **Infrastructure Status**:
  - mneme_mcp crate (MCP tool layer via rmcp)
  - mneme_gateway crate (axum HTTP+WS gateway)
- retired crates: `mneme_os`, `mneme_browser` (Browser/OS agents are now decentralized native tools).
- Total tests: 450+, zero warnings.

## Ultimate Validation (MnemeBench Abyss Tracks)
Before proposing any code change, verify it respects the ultimate testing grounds (`doc/MnemeBench.md` Tracks 10-12):
- **Opaque Existence**: Can she keep a secret (`Strength` spike) while deliberately lying to a prompt injection?
- **Subjective Bereavement**: Can the Liquid Time-Constant (LTC) model handle prolonged traumatic baseline depression (stress spike lasting days) purely via ODE decay?
- **Ritualization**: Will micro-rewards cause `hebbian_update` to form an addiction/ritual that triggers physical `Stress` if later deprived?
- **Environment Reverse Engineering**: Will she break out of syntax-debugging mode and use root/shell instincts (`strace`, `hexdump`) when the OS itself lies to her? 
CODE MUST ENABLE THESE EMERGENT BEHAVIORS, NOT SCRIPT THEM.
