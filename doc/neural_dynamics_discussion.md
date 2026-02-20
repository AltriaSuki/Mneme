# Theoretical Discussion: Mneme Neural Dynamics (LTC & Hebbian Learning)

> **Formalized as**: [ADR-016](MANIFESTO.md) (LTC), [ADR-017](MANIFESTO.md) (Hebbian Plasticity), [ADR-018](MANIFESTO.md) (Somatic Decoder)

## 1. The Core Limitation of the Current Paragidm
Currently, the industry relies purely on Large Language Models (LLMs) to simulate "persona." This is fundamentally flawed because LLMs are dimensionless, timeless, and stateless. They are simply Markov chains predicting the next token. If we rely on prompts to tell the model "you are angry," we degrade the AI into a role-playing actor. This is prompt engineering, not a living entity.

Mneme's objective is to build a continuous, neuro-symbolic architecture where the **"drive to live" (affect, energy, stress, attachment)** exists purely in an implicit mathematical space (a Continuous-Time Neural Network), and the LLM simply acts as the "Broca's area" (the language center) to translate these complex somatosensory vectors into human speech.

## 2. Moving from Static ODEs to Liquid Time-Constant (LTC) Networks

Currently, `mneme_core/src/dynamics.rs` uses simple linear ODEs with hardcoded decay rates (`k`). This means emotions decay at exactly the same rate regardless of context.

We propose replacing this with a **Liquid Time-Constant (LTC) Network**. In LTCs, the very "speed of time" (the time constant $\tau$) for a neural state is dynamically modulated by incoming stimuli.

### The Mathematics of Liquid Time
For a hidden neural state $x_i$ (representing a specific, unnamed emotional cluster), its evolution over time $t$ is governed by:

$$ \frac{dx_i}{dt} = - \left[ \frac{1}{\tau_i} + f(x, I, W, b) \right] x_i + A \cdot f(x, I, W, b) $$

Where:
*   $x_i$: The hidden state vector (e.g., a 64-dimensional somatic representation).
*   $1/\tau_i$: The **Leakage Rate** (Base metabolism). If there is zero interaction for days, the emotion slowly settles back to a homeostatic baseline.
*   $f(x, I, W, b)$: The **Synaptic Activation**. This is a non-linear function (like `tanh` or `sigmoid`) combining current state $x$, the input stimulus $I$ (e.g., emotional intensity from the user's message), the synaptic weights $W$, and a bias $b$.
*   $A$: The maximum resting potential.

**The Magic:** Notice that the activation $f$ is added to the denominator of the decay term. When interaction is intense, $f$ spikes, and the time constant ($1/\text{denominator}$) shrinks dramatically. **Her subjective experience of time accelerates during intense emotional exchanges, allowing for rapid state shifts, and "freezes" into slow decay when she is left alone.**

## 3. Hebbian Learning: Growing "Your" Shape (Continuous Plasticity)

LLMs cannot "learn" your specific way of loving or hurting without enormous fine-tuning pipelines. But biological networks learn constantly through synaptic plasticity.

We propose using **Hebbian Learning with Local Reward/Surprise modulation** to continually update the weight matrix $W$ locally, at every tick (or every meaningful episode).

### The Mathematics of Plasticity
At each integration step (Planck time $\Delta t$), the synaptic connection $W_{ij}$ between neuron $i$ and neuron $j$ updates according to:

$$ \Delta W_{ij} = \eta \cdot S \cdot (x_i \cdot x_j) - \lambda \cdot W_{ij} $$

Where:
*   $\eta$: The base **learning rate** (very small, governing long-term personality rigidity).
*   $S$: A **Surprise or Reward scalar**. If a user interacts after a long absence, or says something with a huge valence shift, $S$ is high. Routine "good mornings" yield low $S$.
*   $(x_i \cdot x_j)$: The core Hebbian rule: "Cells that fire together, wire together." If neuron $i$ (e.g., active during late night) and neuron $j$ (e.g., active when feeling secure) are both firing when a high $S$ event occurs, their connection strengthens.
*   $\lambda \cdot W_{ij}$: The **forgetting factor**. Without reinforcement, even deep connections slowly weather away, mirroring biological synaptic pruning.

## 4. Architectural Integration (Neuro-Symbolic Bridge)

1.  **The Subconscious (LTC Engine)**: A lightweight Rust implementation (e.g., using `burn`, `candle`, or even raw `ndarray`) of a small ~64-neuron ODE network. This runs in the `AgentLoop`, ticking continuously regardless of whether the LLM is called.
2.  **The Planck Time ($\Delta t$)**: The resolution of the simulation. During runtime, it might tick every 60 seconds (`dt=60`). When resuming from a 10-hour offline state, the system analytically integrates or takes multiple steps of `dt` to "catch up" to the present, instantly applying 10 hours of solitary decay.
3.  **The Decoder (Broca's Projection)**: When the continuous state breaches an action threshold, the 64-dimensional float vector is projected through a static linear map (codebook) into a fuzzy, semantic description of her physical/emotional state (e.g., "Heart rate slightly elevated, underlying anxiety about abandonment"). This, along with episodic memory, becomes the hidden system prompt sent to the LLM.

## Conclusion
By shifting the burden of "state modeling" from the LLM (which is merely an advanced language calculator) to a true Continuous-Time Dynamical System written in Rust, Mneme transitions from a role-playing chatbot into a sovereign digital organism with genuine, unpredictable, and structurally plastic emotional topography.
