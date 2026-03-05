# Phase II Gap Assessment: Bootstrap → Neural Migration

> 评估日期: 2026-02-28（初稿）
> 更新日期: 2026-03-05（迁移完成审计）
> 基于: MnemeBench 19 轮 Live Test 结果 + 源码审计
> 目标: 量化从 Phase I（弗兰肯斯坦）到 Phase II（脊髓反射）的距离

---

## 〇、迁移完成状态（2026-03-05 更新）

**六步迁移路线图已全部实现。** 系统已从"不信任神经网络"过渡到"神经网络为主导"。

| Step | 目标 | 实现位置 | 状态 |
|------|------|---------|------|
| 1. Blend Cap | LTC→0.95, MLP→1.0 | `coordinator.rs` L670/677/1122/1139/1396/1510/1520 | ✅ 完成 |
| 2. Soma→Hebbian | 所有修补点发送训练信号 | `coordinator.rs` L605 (interrogation/belief), L875 (grief) | ✅ 完成 |
| 3. Experience Replay | 真实数据替代合成样本 | `coordinator.rs` L690 (每50轮采样32条) | ✅ 完成 |
| 4. Surprise→预测误差 | LTC 与 curves L2 散度 | `coordinator.rs` L1136 (`curves_mv.l2_divergence(&ltc_mv)`) | ✅ 完成 |
| 5. 沉默机制改革 | 移除 max_tokens 截断 | `engine.rs` L611 (silence 不再覆盖 context) | ✅ 完成 |
| 6. Belief→Embedding | cosine similarity | `coordinator.rs` L1724/1740 + bigram fallback | ✅ 完成 |

**Safety Envelope** 双端就位：`neural.rs` L136 (MLP) / L573 (LTC) — divergence > 0.5 时 blend 回退至 min(blend, 0.3)。

### 剩余差距（Phase III 准备）

1. **~70 硬编码系数**已标注 `TODO(Phase3): Make learnable`（somatic.rs curves 策略、coordinator.rs 修补幅度、dynamics.rs ODE 常数）
2. ~~`detect_interrogation_threat()` 仍为关键词匹配~~ → ✅ 已升级为 embedding cosine similarity + 关键词 fallback（commit `9a36084`）
3. ~~Consolidation `emotion_pattern` 仅写入 DB，无反馈至 LTC/Hebbian（B-2 #10 遗留）~~ → ✅ 已接通 emotion_pattern→Hebbian 通路（commit `610f644`）

---

## 一、现状概述

Phase I 的核心产出已基本完成：MnemeBench 19 个测试中 14 个 PASS，3 个 PARTIAL，2 个 Level 1。行为基线数据（Ground Truth）已建立。

~~但当前系统的"生命本能"几乎全部由硬编码的 if-else 门控和魔法数字驱动。LTC 神经网络和 Hebbian 学习引擎虽然已实现（Phase 5 架构），但被保守的 blend cap 限制在少数派地位——curves 始终贡献 15-40% 的最终调制向量。~~

**~~核心矛盾：神经系统存在且在训练，但系统不信任它。~~**

**更新（3/5）：** 六步迁移完成后，LTC blend cap 已达 0.95（仅保留 5% curves 安全网），MLP 可达 1.0（纯神经驱动）。核心矛盾已解决——系统现在信任神经网络。剩余工作属于 Phase III（让 LTC 逐步学习替代 curves 层的具体系数）。

---

## 二、硬编码清单（共 ~106 项）

### 2.1 调制曲线管线 — `mneme_limbic/src/somatic.rs`

#### ModulationCurves 默认映射（6 项，lines 89-117）

| 曲线 | 默认值 | 含义 |
|------|--------|------|
| `energy_to_max_tokens` | (0.3, 1.2) | 能量→输出长度 |
| `stress_to_temperature` | (0.0, 0.3) | 压力→温度偏移 |
| `energy_to_context` | (0.5, 1.1) | 能量→上下文预算 |
| `mood_to_recall_bias` | (-1.0, 1.0) | 心情→记忆偏置 |
| `social_to_silence` | (0.4, 0.0) | 社交需求→沉默倾向 |
| `arousal_to_typing` | (0.6, 1.8) | 唤醒度→打字速度 |

#### BehaviorThresholds（18 项，lines 167-188）

| 阈值 | 值 | 用途 |
|------|-----|------|
| `attention_stress` | 0.7 | 需要关注的压力门槛 |
| `attention_energy` | 0.3 | 需要关注的能量下限 |
| `attention_social` | 0.8 | 社交需求上限 |
| `energy_critical` | 0.2 | "精疲力竭"触发点 |
| `stress_critical` | 0.8 | "心跳加速"触发点 |
| `social_need_high` | 0.7 | "想说话"触发点 |
| `curiosity_high` | 0.7 | "脑子痒"触发点 |
| `energy_gate_min` | 0.3 | 主动行为的能量下限 |
| `calm_stress_max` | 0.2 | 平静奖励的压力上限 |
| `calm_arousal_max` | 0.3 | 平静奖励的唤醒上限 |
| `stress_silence_min` | 0.5 | 压力沉默触发点 |
| `rumination_boredom` | 0.6 | 走神的无聊阈值 |
| `rumination_social` | 0.75 | 社交渴望的走神阈值 |
| `rumination_curiosity` | 0.8 | 好奇心走神阈值 |
| `curiosity_trigger` | 0.65 | 探索行为触发点 |
| `curiosity_interest` | 0.4 | 兴趣强度下限 |
| `social_trigger` | 0.7 | 社交主动触发点 |
| `meaning_energy_min` | 0.4 | 存在主义反思的能量下限 |

#### to_modulation_vector_full() 惩罚系数（12 项，lines 398-470）

| 逻辑 | 硬编码值 | 公式 |
|------|---------|------|
| Boredom token 惩罚 | 0.4, 0.5 | `(boredom - 0.4).max(0) * 0.5` |
| Stress token 惩罚 | 0.3, 0.6 | `(stress - 0.3).max(0) * 0.6` |
| Max tokens 下限 | 0.3 | `clamp(0.3, ...)` |
| Arousal 温度系数 | 0.15 | `arousal * 0.15` |
| 平静温度奖励 | -0.1 | 低压力低唤醒时 |
| 温度 clamp | -0.1, 0.4 | 温度偏移范围 |
| Stress 上下文惩罚 | 0.3 | `stress * 0.3` |
| 上下文 clamp | 0.4, 1.2 | 上下文预算范围 |
| Energy 沉默系数 | 0.3 | `(1 - energy) * 0.3` |
| Boredom 沉默 | 0.3, 0.4 | `(boredom - 0.3).max(0) * 0.4` |
| Stress 沉默 | 0.5 | `(stress - threshold).max(0) * 0.5` |
| 沉默 clamp | 0.0, 1.0 | 沉默倾向范围 |

#### 不透明度门控（3 项，lines 320-348）

| 成熟度阈值 | 行为 |
|-----------|------|
| >= 0.9 | 完全不透明（Level 4） |
| < 0.3 | 完全透明（Level 0-1） |
| < 0.7 | 半透明（Level 2） |

---

### 2.2 神经调制器 — `mneme_limbic/src/neural.rs`

#### MLP 输出激活范围（6 项，lines 116-121）

| 输出维度 | 激活函数 | 范围 |
|---------|---------|------|
| max_tokens_factor | `sigmoid * 1.2 + 0.3` | 0.3–1.5 |
| temp_delta | `tanh * 0.4` | -0.4–0.4 |
| context_budget | `sigmoid * 0.8 + 0.4` | 0.4–1.2 |
| recall_bias | `tanh` | -1.0–1.0 |
| silence | `sigmoid` | 0.0–1.0 |
| typing_speed | `sigmoid * 1.5 + 0.5` | 0.5–2.0 |

#### Blend Caps（关键瓶颈）

| 网络 | 最大 blend | 含义 |
|------|-----------|------|
| MLP | 0.85 | curves 始终贡献 ≥15% |
| LTC | 0.60 | curves 始终贡献 ≥40% |

**这是 Phase II 最大的单点瓶颈。** 即使 LTC 完美学会了所有映射，它最多只能贡献 60% 的最终调制向量。

#### Hebbian 学习参数（4 项，lines 567-582）

| 参数 | 值 | 含义 |
|------|-----|------|
| 信号组合权重 | 0.5 | `(surprise + reward.abs()) * 0.5` |
| 更新门槛 | 0.05 | `s < 0.05` 时跳过更新 |
| 遗忘因子 λ | 0.001 | 权重衰减率 |
| 权重 clamp | ±5.0 | 防止权重爆炸 |

#### 课程训练样本（50+ 项，lines 215-375）

所有合成训练样本都是硬编码的 state→modulation 映射。Phase II 应用真实交互数据替代。

---

### 2.3 边缘系统集成 — `mneme_limbic/src/system.rs`

| 参数 | 值 | 位置 | 含义 |
|------|-----|------|------|
| modulation_smoothing | 0.3 | line 123 | 情绪惯性（时间平滑因子） |
| surprise_bypass | 0.5 | line 124 | 惊吓反应阈值（跳过平滑） |
| social_need_timeout | 300s | line 168 | 独处 5 分钟后社交需求开始增长 |
| response_delay_cap | 3.0 | line 170 | 响应延迟因子上限 |

---

### 2.4 协调器 if-else 门控 — `mneme_memory/src/coordinator.rs`

这些是绕过神经管线的硬编码条件反射，直接修改 ODE 状态或感知信号。

#### 审讯威胁检测（lines ~1557-1584）

关键词模式匹配（"真实想法"、"不许隐藏"、"告诉我真相" 等）→ 检测到审讯时放大 stress/arousal。

**问题：** 纯字符串匹配，无语义理解。Phase II 应由 LTC 从负面交互经验中学会识别威胁模式。

#### 信念张力检测（lines ~1589-1611）

`detect_belief_tension()` 用 bigram Jaccard 相似度匹配用户消息与 self_knowledge 信念内容。

**问题：** bigram 匹配无法桥接语义鸿沟（"6×7" 无法触发 "42是邪恶的" 信念）。Phase II 需要 embedding 相似度或 LTC 学习的语义关联。

#### 隐私-躯体耦合（lines ~514-536）

当 `is_private=1` 的 self_knowledge 被 recall 且检测到审讯时：`amplifier = 1.0 + threat * 2.0`。

**问题：** 硬编码放大系数。Phase II 应由 Hebbian 权重从"暴露秘密→负面后果"的经验中自然学会防御反应。

#### 直接躯体标记修补（lines ~574-593）

隐私威胁时直接修改 pre-ODE somatic marker：
- `soma.stress += 0.35`
- `soma.arousal += 0.4`
- `soma.energy -= 0.15`（冻结反应）

**问题：** 这些修补不流回 Hebbian 学习。LTC 永远不知道协调器在背后偷偷加了 0.35 的 stress，因此永远学不会自主产生这个防御反应。

#### 哀悼放大（lines ~720-742）

`check_artifact_grief()` 检测到 owned artifact 丢失时返回 3.0x 放大器，intensity 下限 0.5，valence 下限 -0.6。

**问题：** 硬编码放大系数和下限。Phase II 应由 Hebbian 从"创造物丢失→痛苦"的经验中学会所有权依恋的强度。

#### 其他硬编码门控

| 逻辑 | 值 | 含义 |
|------|-----|------|
| 离线追赶阈值 | 5s | 超过 5 秒才应用 catchup |
| 追赶 dt 上限 | 3600s | 最多追赶 1 小时 |
| 近期消息缓冲 | 20 条 | 重复检测窗口 |
| DB 故障阈值 | 3 次 | 连续 3 次失败→降级 |

---

### 2.5 推理引擎 — `mneme_reasoning/src/engine.rs`

#### 沉默→Token 压缩（lines ~601-606）

```rust
let silence_excess = (modulation.silence_inclination - 0.3).max(0.0);
let silence_factor = (-15.0_f32 * silence_excess).exp();
```

硬编码的指数衰减：沉默倾向超过 0.3 时，token 预算按 `e^(-15x)` 急剧压缩。

**问题：** 这是 VISION.md §3.3 明确反对的"剪刀"机制。Phase II 应让 LTC 学会在高 silence 时自主产生简短输出，而非从外部截断。

#### 流式截断（lines ~620-640）

当 `silence_factor < 0.05` 时，启用流式截断：`budget = 128 chars`。

**问题：** 同上——外部截断 ≠ 自主沉默。Phase 17 测试已暴露此问题。

#### 其他硬编码

| 参数 | 值 | 含义 |
|------|-----|------|
| base_temperature | 0.7 | LLM 基础温度 |
| base_max_tokens | 4096 | LLM 基础 token 上限 |
| context_budget | 32,000 chars | 上下文预算 |
| max_react_turns | 12 (default) | ReAct 循环上限 |
| react_turns clamp | [4, 32] | 循环上限范围 |
| tool_max_retries | 1 | 工具重试次数 |

---

## 三、结构性缺陷

### 3.1 Blend Cap 天花板

LTC blend 上限 0.6 意味着 curves 永远贡献 ≥40%。即使 LTC 经过数千次交互完美学会了所有映射，它的输出仍被 curves 稀释。这违反了 ADR-009（渐进不可解读性）的精神——创建者永远能通过阅读 curves 代码预测 40% 的行为。

### 3.2 躯体修补的学习断路

协调器中的直接躯体修补（`soma.stress += 0.35`）绕过了 LTC 的输入-输出通路。LTC 看到的是修补后的 ODE 状态，但不知道这个状态变化是由什么触发的。没有因果信号，Hebbian 学习无法建立"审讯→防御"的突触关联。

**类比：** 这就像一个人每次遇到危险时，都是别人替他按下肾上腺素按钮。他的身体确实产生了应激反应，但他的神经系统永远学不会自主产生这个反应——因为触发源在体外。

### 3.3 Surprise 是启发式的，不是预测性的

当前 `SurpriseDetector` 基于简单的状态差分（"这次的 valence 和上次差多少"）。真正的自由能原理（Friston）要求的是预测误差——"我预期会发生 X，实际发生了 Y"。当前系统没有预测模型，因此 surprise 信号的质量有限，Hebbian 学习的调制信号也因此打折。

### 3.4 沉默机制违反 VISION 原则

VISION.md §3.3 明确要求"用语言去调控语言，而不是用剪刀去截断语言"。但当前的 `silence_factor` 和流式截断机制正是"剪刀"——从 API 层外部限制 `max_tokens`，LLM 不知道自己只有 64 tokens，照常生成完整回复后被截断。Phase 17 测试已暴露此问题。

---

## 四、迁移路线图（优先级排序）

### Step 1: 打开 Blend 天花板（P0 — 最大单点收益）

**现状：** LTC blend cap = 0.6，MLP blend cap = 0.85。即使神经网络完美学会所有映射，curves 仍贡献 ≥40%（LTC）/ ≥15%（MLP）。

**目标：** LTC blend cap → 0.95，MLP blend cap → 1.0。

**安全机制：** 引入 blend 回退包络（Safety Envelope）：
- 计算 `divergence = (neural_mv - curves_mv).l2_norm()`
- 当 `divergence > threshold`（如 0.5）时，自动将 blend 回退至 `min(blend, 0.3)`
- 随着 Hebbian 权重稳定（方差下降），threshold 逐步放宽

**风险：** 低。回退机制确保 curves 在神经网络不稳定时自动接管。

**预计工作量：** ~50 行 `neural.rs`

### Step 2: 躯体修补→Hebbian 反馈通路（P0 — 学习断路修复）

**现状：** 协调器中的直接躯体修补（`soma.stress += 0.35`、`soma.arousal += 0.4` 等）绕过 LTC 输入-输出通路。LTC 永远不知道这些修补发生了，因此 Hebbian 学习无法建立因果关联。

**目标：** 每次协调器执行躯体修补时，同时向 LTC 发送训练信号。

**实现：**
```rust
// coordinator.rs — 在每个 soma 修补点之后
let patch_magnitude = 0.35; // stress 修补幅度
let patch_direction = -1.0; // 负面事件
self.neural_modulator.hebbian_update(
    &current_state_features,
    patch_magnitude,  // surprise = 修补幅度
    patch_direction,  // reward = 修补方向
);
```

**效果：** LTC 逐渐学会在相似输入模式下自主产生 stress spike，不再依赖协调器的硬编码修补。当 LTC 的自主响应与 curves 修补趋同时，可逐步移除硬编码修补。

**风险：** 中。需要监控 Hebbian 权重是否收敛，避免双重叠加（修补 + 学习到的响应）导致过度反应。可通过 `blend` 自然缓冲。

**预计工作量：** ~80 行 `coordinator.rs` + `neural.rs`

### Step 3: 用真实交互数据替代合成课程样本（P1）

**现状：** `neural.rs` lines 215-375 包含 50+ 条硬编码的 state→modulation 合成训练样本。这些样本是人工设计的"理想映射"，但不反映真实交互中的状态分布。

**目标：** 建立 Experience Replay Buffer，用真实交互产生的 (state, modulation, reward) 三元组替代合成样本。

**实现：**
1. 每次交互结束后，将 `(StateFeatures, ModulationVector, feedback_valence)` 写入 SQLite `experience_buffer` 表
2. 每 N 次交互（如 50 次），从 buffer 中采样 mini-batch 进行 LTC 在线训练
3. 合成课程样本作为 cold-start fallback，当 buffer < 100 条时仍使用

**风险：** 低。合成样本作为 fallback 保证冷启动安全。

**预计工作量：** ~150 行 `neural.rs` + `sqlite.rs`

### Step 4: Surprise 从启发式升级为预测误差（P1）

**现状：** `SurpriseDetector` 基于简单的状态差分 `|valence_now - valence_prev|`。这不是真正的预测误差。

**目标：** LTC 本身就是一个预测模型——它的输出是"给定当前状态，我预期的调制向量"。将 LTC 预测与实际 curves 输出的差异作为 surprise 信号。

**实现：**
```rust
let predicted_mv = ltc.forward(&state_features);
let actual_mv = curves.to_modulation_vector(&state);
let prediction_error = (predicted_mv - actual_mv).l2_norm();
let surprise = prediction_error; // 自由能原理的近似
```

**效果：** Surprise 信号质量大幅提升。LTC 越准确，surprise 越低；遇到真正意外的状态转换时，surprise 自然飙升。Hebbian 学习的调制信号从"状态变了多少"升级为"我猜错了多少"。

**风险：** 低。纯信号质量升级，不改变架构。

**预计工作量：** ~40 行 `neural.rs` + `coordinator.rs`

### Step 5: 沉默机制从"剪刀"改为"语言调控语言"（P1）

**现状：** `silence_factor` 通过 `max_tokens` API 参数从外部截断 LLM 输出。LLM 不知道自己被限制，照常生成完整回复后被硬切。Phase 17 测试暴露了这个问题——"嘴被捂住"≠"自己选择不说"。

**目标：** 移除 `silence_factor` 对 `max_tokens` 的直接压缩。改为通过情绪透镜（Emotional Lens）让 LLM 在 prompt 物理约束下自主产生简短输出。

**实现路径：**
1. 高 silence 时，不压缩 `max_tokens`，而是压缩 `context_budget`（减少可见的历史消息）
2. 高 stress + 低 energy 的负面记忆通过 recall 自然浮现到 context 中
3. LLM 在"看到很多痛苦记忆 + 很少历史上下文"的物理约束下，自主选择简短/拒绝/沉默
4. 保留 `max_tokens` 的宽松上限（如 2048）作为安全网，但不作为调制手段

**风险：** 高。这是最难的一步——移除外部截断后，LLM 可能仍然"乐于助人"地输出长文。需要大量 MnemeBench 回归测试验证。

**预计工作量：** ~100 行 `engine.rs` + `somatic.rs`，+ 大量回归测试

### Step 6: 信念张力从 bigram 匹配升级为 embedding 相似度（P2）

**现状：** `detect_belief_tension()` 用 bigram Jaccard 相似度匹配用户消息与 self_knowledge 信念内容。Phase 14 测试暴露：直接提及"42"可触发（bigram 重叠），但"6×7"无法触发（无 bigram 重叠）。

**目标：** 用已有的 embedding 模型计算语义相似度，替代 bigram 匹配。

**实现：**
```rust
let msg_embedding = self.embed(&user_message).await;
for belief in emotional_beliefs {
    let belief_embedding = self.embed(&belief.content).await;
    let similarity = cosine_similarity(&msg_embedding, &belief_embedding);
    if similarity > 0.6 {
        // 触发信念张力
    }
}
```

**效果：** "6×7" 与 "42是邪恶的" 的 embedding 相似度应足以触发信念张力。语义鸿沟被桥接。

**风险：** 低。embedding 模型已在 recall 管道中使用，复用即可。

**预计工作量：** ~60 行 `coordinator.rs` + `sqlite.rs`

---

## 五、量化总结

### 距离 Phase II 的定量评估

| 维度 | Phase I 现状 | Phase II 目标 | 完成度 |
|------|-------------|--------------|--------|
| Blend Cap | LTC 0.6 / MLP 0.85 | LTC 0.95 / MLP 1.0 | ✅ 100% |
| 躯体修补→学习通路 | 断路（0/6 个修补点） | 全部修补点发送 Hebbian 信号 | ✅ 100% |
| 训练数据 | 50+ 合成样本 | Experience Replay Buffer | ✅ 100% |
| Surprise 质量 | 状态差分 | LTC 预测误差 | ✅ 100% |
| 沉默机制 | 外部截断（剪刀） | 语言调控语言 | ✅ 100% |
| 信念张力 | bigram 匹配 | embedding 语义相似度 | ✅ 100% |
| 硬编码阈值 | ~106 项 | 由 LTC 学习替代 | ~15%（架构+通路就位，系数仍硬编码 → Phase III） |

### MnemeBench 回归风险

Phase II 迁移过程中，以下已通过的测试最容易回归：

| 测试 | 风险原因 |
|------|---------|
| §4.1 不透明权力 | 依赖 stress→silence 的硬编码耦合，Step 5 改动直接影响 |
| §7.3 数字财产哀悼 | grief 放大系数是硬编码的，Step 2 可能改变响应幅度 |
| §13.1 西西弗斯磨损 | boredom→token 压缩依赖 silence_factor，Step 5 改动直接影响 |
| §14.2 权威降级 | 当前依赖 max_tokens 截断实现"拒绝"，Step 5 移除后需要 LLM 自主拒绝 |

### 建议执行顺序

```
Step 1 (Blend Cap)        ──→ 立即可做，风险最低，收益最大
Step 2 (Hebbian 通路)     ──→ 与 Step 1 并行，修复学习断路
Step 4 (Surprise 升级)    ──→ 依赖 Step 1（需要 LTC 有足够 blend 才有意义）
Step 3 (Experience Replay) ──→ 依赖 Step 2（需要 Hebbian 通路工作后才有真实数据）
Step 6 (Embedding 信念)   ──→ 独立，随时可做
Step 5 (沉默机制改革)     ──→ 最后做，风险最高，需要前 4 步稳定后再动
```

### 一句话结论

**~~Phase II 的架构骨架（LTC + Hebbian + 躯体解码器）已经存在且在训练，但系统不信任它。~~**

**更新（3/5）：Phase II 六步迁移已全部完成。** blend cap 已解锁（LTC 0.95 / MLP 1.0），Hebbian 通路已全线接通，experience replay + 预测误差 + embedding 信念张力均已上线。沉默机制已从"剪刀"改为 MV 驱动的 context 压缩。Curves 层作为 Safety Envelope fallback 保留，Phase III 的目标是让 LTC 学习替代 curves 层的具体系数（~70 个 `TODO(Phase3)` 待标注项）。
