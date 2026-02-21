# Mneme 开发路线图 (Roadmap)

本文档记录当前已知问题和未来改进计划。

> **核心哲学见 [`MANIFESTO.md`](MANIFESTO.md)。** 本文档是工程执行层面的任务清单。
> 所有设计决策的「为什么」记录在 Manifesto 的 ADR 章节。

---

## 🧬 核心设计原则

> **每个 Mneme 都是独特的个体。**

### 从规则到涌现

初始阶段可以使用写死的规则作为 bootstrap，但长期目标是让所有行为参数都通过**学习和经历**形成，而非预设：

| 阶段 | 方式 | 举例 |
|------|------|------|
| **Bootstrap** | 硬编码规则 | `if energy < 0.3 { 回复简洁 }` |
| **过渡期** | 规则 + 可调参数 | 阈值 0.3 变为可学习的 `energy_threshold` |
| **成熟期** | 完全数据驱动 | 神经网络直接从状态映射到行为倾向 |

### 两类改进

在阅读本文档时，请区分：

| 类型 | 说明 | 举例 |
|------|------|------|
| 🏗️ **基础设施** | 所有 Mneme 共享的底层能力 | API 重试、数据库、CLI |
| 🧬 **个性参数** | 每个 Mneme 独特的可学习部分 | 阈值、偏好、表达风格 |

基础设施的改进是确定性的；个性参数的改进目标是**消除确定性**，让每个实例都不同。

### 个体差异来源

每个 Mneme 实例应该因为以下因素而不同：

1. **经历** - 不同的对话历史塑造不同的记忆和叙事
2. **反馈** - 用户的反应强化不同的行为模式
3. **价值演化** - 价值网络权重随时间漂移
4. **依恋关系** - 与不同用户形成不同的依恋风格
5. **叙事偏差** - 解读世界的方式因经历而异
6. **表达阈值** - 什么状态下该简洁/详细/热情/冷淡

### 当前硬编码清单（待消除）

以下是目前代码中硬编码的规则，需要逐步替换为可学习参数：

| 位置 | 硬编码内容 | 目标 | 优先级 |
|------|-----------|------|--------|
| `dynamics.rs` | `energy_target: 0.7` | 从反馈中学习最优目标 | ✅ `LearnableDynamics` |
| `dynamics.rs` | `stress_decay_rate: 0.005` | 个体化的压力恢复速度 | ✅ `LearnableDynamics` |
| `somatic.rs` | `energy < 0.3 → 简洁回复` | 学习什么状态下该简洁 | ✅ `BehaviorThresholds` |
| `somatic.rs` | `stress > 0.7 → 语气略急` | 学习压力如何影响表达 | ✅ `BehaviorThresholds` |
| `state.rs` | 行为指导文本 `describe_for_context()` | 删除（ModulationVector 已替代）→ 审计 B-1 | ✅ |
| `state.rs` | `ValueNetwork::default()` 预设道德权重 | 空初始化 → 从 self_knowledge 加载 → 审计 B-1 | ✅ |
| `values.rs` | 初始价值权重 | 从用户反馈中调整 | 🟢 |
| `prompts.rs` | 工具说明规定性语气 | 改为中性技术规范 → 审计 B-14 | ✅ |
| `prompts.rs` | ~~工具说明硬编码~~ | 工具通过 API native tools 参数传递，不再在 prompt 中重复；已删除死代码 | ✅ |
| `tools.rs` | ~~固定 6 工具列表~~ | shell 为唯一硬编码工具（身体器官），其余通过 MCP 动态获取 → 审计 B-8 | ✅ |
| `persona/*.md` | 行为处方式种子文件 | 改为纯事实性种子记忆 → 审计 B-2 | ✅ |
| `engine.rs` | `sanitize_chat_output()` 固定过滤 | 查询 self_knowledge 表达偏好 → 审计 ADR-007 | ✅ |
| `somatic.rs` | ~~**文字指令注入模式**~~ | ~~→ 结构性调制（无状态 LLM 范式）~~ | ✅ |
| `engine.rs` | ~~`max_tokens` 固定~~ | ~~→ 由 energy/stress 调制~~ | ✅ |
| `engine.rs` | ~~`temperature` 固定~~ | ~~→ 由 arousal/stress 调制~~ | ✅ |
| `engine.rs` | ~~记忆召回无偏差~~ | ~~→ 由 mood/stress 偏置 recall~~ | ✅ |

### Manifesto 合规审计（2026-02-13 更新）

以下是对照 `MANIFESTO.md` 核心信念和 ADR 的代码审计结果：

#### 已修复的违规

| 违规 | 位置 | Manifesto 条款 | 说明 |
|------|------|----------------|------|
| `describe_for_context()` 硬编码行为指令 | `state.rs` | B-1 存在先于本质 | ✅ 已删除，由 ModulationVector 结构性调制 |
| `ValueNetwork::default()` 预设道德权重 | `state.rs` | B-1 存在先于本质 | ✅ 空初始化 |
| persona/*.md 含行为处方 | `persona/broca.md` 等 | B-2 Persona 是输出不是输入 | ✅ 重写为事实性种子记忆 |
| `sanitize_chat_output()` 固定过滤器 | `engine.rs` | ADR-007 表达自由 | ✅ `ExpressionPreferences` 从 self_knowledge 读取 |
| 工具说明使用规定性语言 | `prompts.rs` | B-14 不回避冲突 | ✅ 中性英文技术规范 |
| 好奇心是标量 | `state.rs` | ADR-007 好奇心方向性 | ✅ `CuriosityVector` 向量化，topic tagging + decay |
| 记忆召回无重建 | `sqlite.rs` | B-10 记忆是重建 | ✅ `recall_reconstructed()` 情绪着色 |
| 社交图谱无信任维度 | `sqlite.rs` | B-19 信任默认存在 | ✅ v0.7.0 添加 `trust_level` 字段 → ⚠️ v0.8.0 移除（B-19 说"信任不是显式数值"）→ 改为 self_knowledge 条目综合效果 |
| 私密条目仍注入 prompt | `prompts.rs` | B-9 不透明涌现 | ✅ v0.7.0 添加 auto-privacy + SQL 过滤 → ⚠️ v0.8.0 移除（B-9 说"不建造 privacy_filter"）→ 所有 self_knowledge 对 LLM 可见，由她自主决定说不说 |
| 用户断言覆写自我认知 | `sqlite.rs` | B-5 认知主权 | ✅ 自源知识抵抗外部覆写 (`cap = min(new, existing * 0.8)`) |
| 无冲突表达机制 | `engine.rs` | B-14 冲突是活物的证明 | ✅ v0.7.0 添加 `detect_input_conflict()` + temperature 调制 → ⚠️ v0.8.0 移除（B-14 说冲突应从 self_knowledge 涌现，不是工程注入）→ 冲突能力依赖 LLM 自主表达 |
| 无习惯检测 | `expression/habits.rs` | B-21 习惯与仪式 | ✅ `HabitDetector` 重复模式检测 + 反思触发 |
| 触发器无优先级竞争 | `expression/attention.rs` | B-17 注意力单线程 | ✅ `AttentionGate` 优先级竞争 + engagement 调制 |

#### 仍存在的违规

| 违规 | 严重度 | 位置 | Manifesto 条款 | 说明 |
|------|--------|------|----------------|------|
| ~~工具列表固定~~ | ~~🟡 中~~ | `tools.rs` | B-8 / ADR-014 | ✅ ShellToolHandler 为唯一硬编码工具，browser 工具已删除，其余通过 MCP 动态获取 |
| ~~无低分辨率独白~~ | ~~🟡 中~~ | — | ADR-013 / B-16 | ✅ 三层分辨率完整：Zero(纯ODE)、Low(Ollama+strength 0.2)、High(完整LLM)；surprise升级+预算感知 |
| ~~无形成性课程~~ | ~~🟡 中~~ | `tools.rs` | ADR-011 | ✅ ReadingToolHandler 阅读 → 状态依赖反思 → self_knowledge |
| 无记忆加密 | 🟢 低 | — | B-12 结构性保障 | Level 0-1 全透明是合理的，Level 2+ 需要可选加密 |
| ~~平台协议侵入核心~~ | ~~🟡 中~~ | `mneme_onebot` | ADR-015 | ✅ `route_message()` 封装协议路由，CLI 不再解析 group_id/user_id |

### 实现路径

> **当前重点见「版本规划 v2.0.0」章节。** 下表为硬编码消除的长期路径。

```
Phase 1: 参数化 ✅ 部分完成
    ModulationVector 已将 max_tokens/temperature/context_budget 参数化
    剩余：recall bias、typing speed、silence threshold
    
Phase 2: 可观测
    记录"规则触发 → 行为 → 用户反馈"的关联
    建立 (state, action, reward) 三元组数据集
    
Phase 3: 在线微调
    用反馈信号调整 ModulationCurves 参数
    每次正面反馈强化当前曲线，负面反馈调整
    
Phase 4: 液体神经架构 (ADR-016/017/018)
    NeuralModulator 升级为液体时间常数网络 (LTC)：τ 受输入强度动态调制
    赫布学习在线权重更新：surprise/reward 调制的局部突触可塑性
    躯体解码器 (Somatic Decoder)：模糊的线性映射取代显式数值注入 prompt
```

### 无状态 LLM：从"指令"到"结构性约束"

> **核心洞察：不要告诉 LLM 如何感受，而要让它在不同状态下结构性地接收不同的输入和约束——行为自然涌现。**

#### 问题：当前方法的根本缺陷

当前架构是"导演模式"——用文字指令告诉 LLM 扮演某种状态：

```
OrganismState(stress=1.0)
    → SomaticMarker → "语气可能略急"
    → 注入 system prompt
    → LLM 读到这句话...然后无视它
    → "挺好的，在和你聊这个项目" 
```

这就像给演员一张纸条写"请表演疲惫"。他可以照做，也可以不看。LLM 没有**结构性的理由**必须遵从这些文字指令。

#### 类比：人脑是怎么做的

人类大脑皮层（neocortex，类比 LLM）是一个相对"无状态"的处理器——它处理接收到的任何输入。情绪和人格**不来自皮层内部**，而来自皮层下结构（边缘系统、脑干）对皮层的**结构性调制**：

| 人脑机制 | 类比 | 当前 Mneme | 目标 Mneme |
|----------|------|------------|------------|
| 疲劳时工作记忆容量下降 | 上下文窗口缩小 | ❌ 告诉 LLM"你累了" | ✅ 实际传更少的 context |
| 焦虑时注意力偏向威胁信息 | 记忆召回偏差 | ❌ 告诉 LLM"你焦虑" | ✅ 实际召回更多负面 episodes |
| 抑郁时行动力下降 | 输出约束 | ❌ 告诉 LLM"语气偏淡" | ✅ 实际限制 max_tokens |
| 兴奋时思维发散 | temperature 调节 | ❌ 告诉 LLM"活泼热情" | ✅ 实际提高 temperature |
| 疲惫时不想说话 | 响应概率 | ❌ 告诉 LLM"简洁回复" | ✅ 实际提高 [SILENCE] 倾向 |

**关键区别**：杏仁核不给前额叶发备忘录——它改变神经递质水平，**物理地改变**信息处理过程。

#### 架构范式转变

**旧范式（导演模式）**：状态 → 文字描述 → 注入 prompt → 期望 LLM 遵从  
**新范式（具身模式）**：状态 → 结构性调制 LLM 的输入/参数/输出 → 行为自然涌现

```
                    ┌─────────────────────────────────────────┐
                    │          OrganismState                   │
                    │   energy, stress, mood, curiosity, ...   │
                    └──────┬──────────┬──────────┬────────────┘
                           │          │          │
                    ┌──────▼──┐ ┌─────▼────┐ ┌──▼──────────┐
                    │ Input   │ │ LLM      │ │ Output      │
                    │ Modula- │ │ Param    │ │ Modula-     │
                    │ tion    │ │ Modula-  │ │ tion        │
                    │         │ │ tion     │ │             │
                    └────┬────┘ └────┬─────┘ └──────┬──────┘
                         │          │               │
              ┌──────────▼──────────▼───────────────▼──────┐
              │                                            │
              │    LLM（无状态纯推理引擎 = "neocortex"）    │
              │    不知道自己"累了"或"开心"                  │
              │    只是处理收到的信息                        │
              │                                            │
              └────────────────────────────────────────────┘
```

#### 三层调制机制

**Layer 1: 输入调制（什么信息到达 LLM）**

状态不是"建议"——它**筛选、过滤、偏置**进入 LLM 的信息：

| 状态维度 | 调制目标 | 机制 | 效果 |
|----------|----------|------|------|
| `energy` | context 窗口大小 | 低精力 → 裁剪更多低优先级 context | 自然产生短回复（因为可用信息少） |
| `stress` | 记忆召回偏差 | 高压力 → recall 偏向负面/威胁相关记忆 | 自然产生防御性/急躁回复 |
| `mood_bias` | 记忆情感过滤 | 低情绪 → 过滤掉积极记忆 | 自然产生低沉语气 |
| `curiosity` | 工具可用性 | 高好奇 → 暴露更多探索工具 | 自然产生主动探索行为 |
| `social_need` | feed digest 注入量 | 高社交需求 → 注入更多话题素材 | 自然产生延续话题行为 |
| `openness` | 外部信息量 | 高开放 → 注入更多新闻/feed | 自然产生发散思维 |

**Layer 2: LLM 参数调制（如何处理信息）**

| 状态维度 | 调制参数 | 机制 | 效果 |
|----------|----------|------|------|
| `energy` | `max_tokens` | 低精力 → 降低 max_tokens | 物理上不可能长篇大论 |
| `stress` | `temperature` | 高压力 → 提高 temperature | 回复更不可预测/冲动 |
| `affect.arousal` | `temperature` | 高唤醒 → 提高 temperature | 表达更激烈 |
| `mood_bias` | `top_p` | 极端情绪 → 降低 top_p | 用词范围收窄 |
| *(保留最小 somatic hint)* | `system prompt` | 仍保留极简的状态数值 | LLM 可选的参考信号 |

**Layer 3: 输出调制（如何呈现结果）**

| 状态维度 | 调制目标 | 机制 | 效果 |
|----------|----------|------|------|
| `energy` | 消息拆分阈值 | 低精力 → 不拆分/单条发送 | 懒得分多条 |
| `affect.arousal` | humanizer 打字速度 | 高唤醒 → 更快发送 | 急迫感 |
| `stress` | 回复延迟 | 高压力 → 随机更长延迟 | 犹豫/回避 |
| `mood_bias` | 模态选择 | 低情绪 → 偏好文字而非语音 | 不想说话 |
| `social_need` | [SILENCE] 阈值 | 低社交需求 → 更容易沉默 | 不想搭理 |

#### 与核心原则的关系

这个范式转变**深化**了"从规则到涌现"的原则：

| 阶段 | 旧框架 | 新框架 |
|------|--------|--------|
| **Bootstrap** | `if stress > 0.7 { 告诉 LLM "语气略急" }` | `stress → max_tokens *= 0.6, temp += 0.2` |
| **过渡期** | 阈值可学习，但仍是文字指令 | 调制曲线可学习：stress 如何映射到 temperature |
| **成熟期** | 神经网络生成文字指令 | 神经网络直接输出 LLM 参数调制向量 |

关键区别：**旧框架的终点仍然是生成文字指令让 LLM 遵从；新框架的终点是根本不需要文字指令**——行为从结构性约束中涌现。

#### 🧬 调制曲线是个性参数

每个 Mneme 实例的调制曲线应该不同——这是另一个个体差异来源：

```
实例 A（敏感型）:
    stress 0.3 → temperature +0.3, max_tokens ×0.5
    （压力稍高就变得急躁简短）

实例 B（坚韧型）:
    stress 0.8 → temperature +0.1, max_tokens ×0.9
    （压力很大才有微小变化）

实例 C（戏剧化型）:
    mood +0.5 → temperature +0.4, context ×1.5
    mood -0.5 → temperature -0.2, max_tokens ×0.3
    （情绪对行为影响极大）
```

这些曲线从反馈中学习：如果用户对某个状态下的回复满意，强化当时的调制参数。

#### SomaticMarker 的角色转变

`SomaticMarker` 已完成职责转变（✅ #20 短期）：

| | 旧角色（v0.2 前） | 当前角色（v0.3+） |
|---|--------|--------|
| **给谁** | 给 LLM 读的文字 | 给架构层的调制信号 |
| **产出** | `"语气可能略急"` | `ModulationVector { temp: +0.2, max_tokens: 0.6, ... }` |
| **LLM 看到** | 行为指导文本 | 极简数值（可选辅助信号） |
| **行为来源** | LLM 解读文字后"演"出来 | 结构性约束下自然涌现 |

---

## 🔴 高优先级 (High Priority)

### 1. ✅ API 错误自动重试机制
**模块**: `mneme_reasoning/src/providers/`  
**问题**: API 调用失败时直接返回错误，没有重试机制。对于临时性网络问题或 rate limit，应该自动重试。

**当前代码**:
```rust
// anthropic.rs - 直接 bail 没有重试
if !response.status().is_success() {
    let error_text = response.text().await.unwrap_or_default();
    anyhow::bail!("Anthropic API Error: {}", error_text);
}
```

**需要实现**:
- [x] 指数退避重试 (exponential backoff) ✅ — `retry.rs` with configurable backoff
- [x] 区分可重试错误 (429 rate limit, 5xx) 和不可重试错误 (400, 401) ✅
- [x] 配置最大重试次数 ✅ — `RetryConfig { max_attempts, initial_delay, max_delay, backoff_factor }`
- [x] 重试日志记录 ✅ — tracing::warn on each retry, tracing::info on success after retry

**参考**: `mneme_onebot/src/client.rs` 已有类似实现

---

### 2. ✅ 工具执行错误处理
**模块**: `mneme_reasoning/src/engine.rs`  
**问题**: 工具执行失败时，错误信息可能不够清晰，且没有重试机制。

**已完成** (commit `fcf5602`):
- [x] 工具执行重试机制（对于临时性失败） ✅ — `execute_tool_with_retry()` + `TOOL_MAX_RETRIES=1`，仅重试 `Transient` 类型错误
- [x] 结构化错误返回 (`ToolResult.is_error`) ✅ — `ToolOutcome { content, is_error, error_kind }` + `ToolErrorKind::Transient/Permanent`
- [x] 工具执行超时可配置化 ✅ — `LocalExecutor::with_timeout()` 已存在，timeout → Transient 自动重试
- [x] 浏览器会话恢复机制 ✅ — `execute_browser_tool()` 失败时 drop session → `create_browser_session()` 重建
- [x] 错误分类：timeout/spawn → Transient（重试），exit code/missing param/unknown tool → Permanent（不重试）
- [x] `is_error: Some(true/false)` 传递给 LLM，让模型知道工具是否失败
- [x] `BrowserAction` derive `Clone` 支持 recovery 重试
- [x] 9 个新测试 + 全部 33 integration tests 通过 ✅

---

### 3. ✅ 状态历史记录（调试与回溯）
**模块**: `mneme_memory/src/coordinator.rs`, `mneme_memory/src/sqlite.rs`  
**问题**: 当前只保存最新状态快照，无法回溯历史状态变化。

**已完成** (commit `8c7ad3f`):
- [x] 状态快照历史表 (`organism_state_history`) ✅ — SQLite migration, timestamp 索引
- [x] 定期记录 (每 N 分钟或每次显著变化) ✅ — tick/interaction/consolidation/shutdown 四种触发器
- [x] 状态 diff 计算与存储 ✅ — `compute_state_diff()` 比较 prev/curr，epsilon=0.01，紧凑格式 `E-0.40 S+0.60`
- [x] 调试时可查询特定时间点的状态 ✅ — `query_state_history(from, to, limit)` + `recent_state_history(count)`
- [x] 自动清理过旧历史（保留策略） ✅ — `prune_state_history(keep=10000, max_age=7d)`，每 360 tick 自动触发
- [x] `StateSnapshot` 结构体 + `prev_snapshot` diff 跟踪 ✅
- [x] 7 个测试（4 unit + 3 integration） ✅

**Schema**:
```sql
CREATE TABLE organism_state_history (
    id INTEGER PRIMARY KEY,
    timestamp INTEGER NOT NULL,
    state_json TEXT NOT NULL,
    trigger TEXT,  -- 'tick', 'interaction', 'consolidation', 'shutdown'
    diff_summary TEXT
);
CREATE INDEX idx_state_history_ts ON organism_state_history(timestamp);
```

---

### 4. ✅ 数值边界检查与 NaN 防护
**模块**: `mneme_core/src/dynamics.rs`, `mneme_core/src/state.rs`  
**问题**: 浮点运算可能产生 NaN 或溢出，目前只有简单的 clamp。

**当前代码**:
```rust
// dynamics.rs - 只有 clamp，没有 NaN 检查
fast.normalize();

impl FastState {
    pub fn normalize(&mut self) {
        self.energy = self.energy.clamp(0.0, 1.0);
        // ... 没有 NaN 检查
    }
}
```

**需要实现**:
- [x] 统一的 `SafeF32` 类型或 validate 宏 ✅ — `deserialize_safe_f32()` serde helper, applied to all f32 fields in FastState/MediumState/AttachmentState/Affect
- [x] 在 `normalize()` 中检测并处理 NaN/Infinity ✅ — `sanitize_f32()` + fallback
- [x] 状态异常时的回退策略（恢复默认值） ✅ — fallback to homeostatic defaults
- [x] 异常状态日志告警 ✅ — `tracing::warn!` on NaN/Inf detection
- [x] 单元测试覆盖边界情况 ✅ — `test_nan_resistance` + `test_extreme_dt_stability`
- [x] `MediumState::normalize()` 同样添加 NaN 防护 ✅

**示例**:
```rust
fn safe_normalize(value: f32, min: f32, max: f32, default: f32) -> f32 {
    if value.is_nan() || value.is_infinite() {
        tracing::warn!("Detected invalid float, resetting to default");
        return default;
    }
    value.clamp(min, max)
}
```

---

### 26. ✅ Semantic Memory 读写闭环
**模块**: `mneme_memory/src/sqlite.rs`, `mneme_memory/src/coordinator.rs`  
**问题**: `semantic_facts` 表已存在于 SQLite schema 中，但**没有任何代码实际读写事实三元组**。当前 `recall()` 只做 episode 向量搜索，不查询 facts、不查询 social graph、不融合 feed digest。agent 只能"检索到说过什么"，不能"知道什么"——记忆系统最核心的价值尚未兑现。

**设计文档 §4.1-4.2 要求 vs 当前状态**:

| 记忆系统 | 状态 | 说明 |
|----------|------|------|
| Episodic Memory | ✅ | 向量搜索可用 |
| Semantic Memory (fact triples) | ✅ | store/recall/decay/format 已实现 |
| Social Memory (人物图谱) | ✅ | trait + 表 + 读写闭环（#53 已修复） |
| Blended Recall | ✅ | `recall_blended()` 混合 episodes + facts + social |

**需要实现**:
- [x] `store_fact()` / `update_fact()` — 写入事实三元组 (subject, predicate, object, confidence) ✅
- [x] `recall_facts()` — 根据主题/关键词召回相关事实 ✅
- [x] 事实冲突检测与更新（重复三元组自动合并 confidence） ✅
- [x] `get_facts_about()` — 按主题查询事实 ✅
- [x] `decay_fact()` — 事实衰减（矛盾信息出现时降低 confidence） ✅
- [x] `format_facts_for_prompt()` — 格式化事实供 prompt 注入 ✅
- [x] 对话后的 fact extraction pass（`extraction.rs` + `extract_facts()` + think() 集成） ✅
- [x] Social graph 读写闭环（`get_person_context()` + `upsert_person()` + `record_interaction()` + engine 集成 + prompt 注入） ✅
- [x] `Coordinator::recall()` 返回混合结果：episodes + facts + social context（`BlendedRecall` + `recall_blended()`） ✅

---

### 27. ✅ Persona 从记忆涌现 (ADR-002)
**模块**: `persona/*.md`, `mneme_core/src/persona.rs`, `mneme_memory/src/sqlite.rs`  
**问题**: Persona 曾是静态 `.md` 文件，硬注入 system prompt。这违背了核心信念 **B-2: Persona 是输出不是输入**（见 `MANIFESTO.md`）。性格应由记忆决定，不由配置文件加载。

**架构变更**:
- `Psyche` 不再从文件加载 5 个 markdown 字段作为运行时配置
- 改为从 `self_knowledge` 表动态构建自我认知（见 #35）
- `persona/*.md` 保留为「出生证明」— 首次启动时解析为种子记忆写入数据库，之后不再读取
- 性格在对话、反思、遗忘中自然演化

**需要实现**:
- [x] 设计并填充 5 个脑区 persona 文件内容 ✅（已设定为"刚出生的小女孩"人格）
- [x] `Psyche` struct 重构：`species_identity` (写死：物种级不变量) + `self_model` (动态：从 self_knowledge 表读取) ✅
- [x] `Psyche::format_context()` 从 `self_knowledge` 表读取，按 confidence 排序拼装 ✅
- [x] 首次启动时 `seed_from_persona_files()` 将 .md 内容解析为种子记忆，写入 self_knowledge 表 ✅
- [x] 后续启动检测到 self_knowledge 非空则跳过 seed ✅
- [x] Sleep consolidation 产出新的 self_knowledge 条目（自我反思步骤，见 #39） ✅
- [x] 废弃 `PersonaLoader` 的文件读取逻辑，迁移完成后可删除 ✅ — 已用 `SeedPersona` 替代

---

### 28. ✅ Context Assembly 完整管道
**模块**: `mneme_reasoning/src/prompts.rs`, `mneme_reasoning/src/engine.rs`  
**问题**: 设计文档 §5.2 定义了 6 层上下文优先级，当前只实现了约 2 层。这是 reasoning 质量的天花板——"The quality of the agent depends entirely on what context reaches the LLM."

**设计文档 §5.2 要求 vs 当前状态**:

| 优先级 | 内容 | 状态 | 说明 |
|--------|------|------|------|
| 1 | Persona 定义 | ✅ | 加载机制有，文件已填充（见 #27） |
| 2 | User facts（语义记忆） | ✅ | `recall_facts_formatted()` 注入 prompt |
| 3 | Social feed digest | ✅ | `format_feed_digest()` + `update_feed_digest()` + CLI sync 写入 |
| 4 | Relevant episodes | ✅ | 向量搜索结果注入 `ContextLayers.recalled_episodes` |
| 5 | Conversation history | ✅ | 已实现，滑动窗口 |
| 6 | Triggering event | ✅ | 已实现 |
| — | Somatic marker | ✅ | 额外注入（设计文档之外的创新） |
| — | Token 预算管理 | ✅ | `context_budget_factor` 调制上下文量 |

**需要实现**:
- [x] 将 `recall_facts()` 结果注入 prompt（user facts 层） ✅
- [x] 实现 feed digest 生成（`format_feed_digest()` → CLI sync → `feed_cache` → `ContextLayers`） ✅
- [x] 将语义相关 episodes 注入 prompt ✅ — `ContextLayers.recalled_episodes`
- [x] Token 预算管理：按优先级裁剪（feed digest 先于 user facts 丢弃；persona 永不丢弃） ✅
- [x] 统一的 `ContextAssembler` 结构体 + `ContextLayers` + `build_full_system_prompt()` ✅

---

### 29. ✅ 安全沙箱与 Capability Tiers
**模块**: `mneme_core/src/safety.rs`, `mneme_reasoning/src/engine.rs`

**已完成** (v0.4.0):
- [x] 三级能力模型 `CapabilityTier`: ReadOnly / Restricted / Full ✅
- [x] `CapabilityGuard` 运行时权限检查（工具执行前拦截） ✅
- [x] 命令黑名单过滤（rm -rf, sudo, mkfs 等破坏性命令） ✅
- [x] 路径沙箱：Restricted 模式下文件操作限制在白名单目录内 ✅
- [x] 网络出站域名白名单 ✅
- [x] ReadOnly 模式下只允许 ls/cat/git status 等只读命令 ✅
- [x] 集成到 `ReasoningEngine.execute_tool()` + `ToolRegistry.dispatch()` ✅
- [x] 通过 `MnemeConfig.safety` 统一配置 ✅
- [x] 15 个测试覆盖三级权限 + 路径 + URL + 黑名单 ✅

**待后续补充**:
- [x] Destructive 操作的用户确认流程（交互式） ✅ `9a7d84c`
- [x] 审计日志：所有工具调用记录 ✅ — ToolRegistry.dispatch() 结构化 tracing 日志 (tool/elapsed_ms/input/outcome)

---

### 45. ✅ Coordinator 并发安全
**模块**: `mneme_memory/src/coordinator.rs`
**优先级**: 🔴 高

**问题 A — 状态更新竞态**:
`trigger_sleep()` 先 `state.read()` 做 consolidation，再 `state.write()` 写回结果。期间 `process_interaction()` 的状态修改会被覆盖丢失。

**问题 B — 多 RwLock 死锁风险**:
`process_interaction()` 先锁 `state` 再锁 `prev_somatic`；其他方法可能以不同顺序获取锁。无统一锁顺序约定。

**需要实现**:
- [x] 状态版本号 → 改用 `state_mutation_lock: tokio::sync::Mutex<()>` 序列化所有状态变更操作 ✅
- [x] 统一锁顺序文档：`state_mutation_lock` → `state` → `prev_somatic` → `episode_buffer` → `feedback_buffer` ✅
- [ ] 或改用 actor/mpsc 模式替代多 RwLock（见 #17 代码组织优化）

---

### 46. ✅ CLI 关机可靠性
**模块**: `mneme_cli/src/main.rs`
**优先级**: 🔴 高

**问题**: 三条关机路径（quit 命令、Ctrl-C readline、Ctrl-C tokio signal）都用 `std::process::exit(0)` + 500ms `sleep` 硬等待。如果 `coordinator.shutdown()` 超过 500ms，数据库写入可能不完整。

**需要实现**:
- [x] 用 `tokio::sync::oneshot` channel 替代 sleep 硬等待 ✅
- [x] `graceful_shutdown()` 带 5s timeout，shutdown 完成后再 exit ✅
- [x] 统一关机路径为一个 async 函数，避免三处重复代码 ✅

---

### 47. ✅ goals.rs / rules.rs 数据库集成验证
**模块**: `mneme_memory/src/goals.rs`, `mneme_memory/src/rules.rs`
**优先级**: 🔴 高

**问题**: 这两个是 untracked 新文件。`GoalManager` 调用 `db.load_active_goals()` / `db.create_goal()` 等方法，`RuleEngine` 调用 `db.load_behavior_rules()`。如果这些方法在 `SqliteMemory` 中未实现，运行时会 panic。

**需要实现**:
- [x] 确认 `SqliteMemory` 已实现所有 Goal/Rule 相关 DB 方法 ✅
- [x] 补充集成测试：GoalManager + SqliteMemory 端到端 ✅
- [x] 补充集成测试：RuleEngine + SqliteMemory 端到端 ✅
- [x] `rules.rs` 触发匹配修复：`discriminant()` 只比较枚举变体，不比较内部数据（如 `field`, `threshold`）→ 完整 pattern matching ✅

---

## 🟡 中优先级 (Medium Priority)

### 5. ✅ 反馈信号收集与持久化
**模块**: `mneme_memory/src/feedback_buffer.rs`, `mneme_memory/src/sqlite.rs`

**已完成**:
- [x] 反馈信号实时持久化到 SQLite ✅
- [x] 启动时加载未整合的信号 ✅
- [x] 用户显式反馈机制 ✅ — like/dislike + `correct <text>` 纠正命令，UserCorrection 信号类型
- [x] 隐式反馈推断 ✅ — 回复延迟(< 30s=正向, > 300s=负向) + 消息长度比(EMA基线) → ImplicitEngagement 信号

---

### 6. 🏗️ 属性测试 (Property-Based Testing) ✅
**模块**: 全局  
**问题**: ~~当前只有基础单元测试，缺乏随机化测试来发现边界问题。~~

**已完成** (41 proptest tests across 3 crates):

| Crate | Tests | 覆盖内容 |
|-------|-------|----------|
| mneme_core | 21 | ODE 任意状态/输入/dt 稳定性, NaN 注入恢复, normalize() 幂等性, Affect 边界/lerp/polar, Emotion roundtrip, 依恋更新边界, 道德成本边界 |
| mneme_limbic | 8 | ModulationVector 6 字段边界验证, energy→max_tokens 单调性, stress→temperature 单调性, SomaticMarker 格式验证, proactivity_urgency 边界 |
| mneme_reasoning | 12 | sanitize_chat_output 幂等性, 任意 Unicode 不 panic, header/bullet/bold 移除, 中文保留, 2000 cases |

**Bug found**: `sanitize_chat_output` 对重叠 `*` 模式 (如 `**0*text*`) 不幂等。已修复。

**待后续补充**:
- [x] 序列化/反序列化往返测试 ✅ OrganismState/Affect/Emotion JSON roundtrip
- [x] 并发安全测试 ✅ — coordinator 多任务交互/反馈/健康监控/睡眠阻塞 4 项

---

### 7. ✅ 浏览器工具稳定性
**模块**: `mneme_browser/src/client.rs`, `mneme_reasoning/src/engine.rs`  
**问题**: headless_chrome 可能不稳定，没有健康检查和会话恢复。

**已完成** (commit `d566528`):
- [x] 迁移 deprecated `wait_for_initial_tab()` → `new_tab()` ✅ — 消除编译警告，headless_chrome v1.0.4+ 推荐方式
- [x] `BrowserConfig` 配置结构体 ✅ — headless 模式、element_timeout、navigation_timeout，`debug()` 构造器用于非 headless 调试
- [x] `is_alive()` 健康检查 ✅ — 通过 `tab.get_target_info()` CDP 调用探测浏览器存活
- [x] `tab()` DRY helper ✅ — 替代所有方法中重复的 `if let Some(tab) = &self.current_tab` 模式
- [x] Proactive session recovery ✅ — `execute_browser_tool()` 执行前检查 `is_alive()`，死亡会话自动丢弃重建
- [x] HTML 截断 ✅ — `GetHtml` action 限制返回 8KB，防止巨型页面撑爆 context
- [x] `set_default_timeout()` ✅ — 新建 tab 时设置可配置超时
- [x] 3 个新单元测试（BrowserConfig default/debug/custom）✅

---

### 8. ✅ LLM 响应解析健壮性
**模块**: `mneme_reasoning/src/engine.rs`, `mneme_reasoning/src/extraction.rs`  
**问题**: LLM 输出格式可能不符合预期，解析可能失败。

**已完成** (commit `5b581df`):
- [x] Emotion tag 解析健壮化 ✅ — `parse_emotion_tags()`: 处理大小写 `<Emotion>`、空格 `< emotion >`、多标签、空内容、无法识别的情绪值
- [x] Silence 检测健壮化 ✅ — `is_silence_response()`: 大小写不敏感、容忍空格/省略号，但不误判含 SILENCE 的正常文本
- [x] JSON 提取多策略解析 ✅ — 6 层 fallback: 直接解析 → code block 提取 → balanced braces → bare array → JSON repair → graceful empty
- [x] JSON repair ✅ — `repair_json()`: 修复 trailing commas、单引号替换、unquoted keys
- [x] `extract_balanced_braces()` ✅ — 正确处理嵌套 `{}`、字符串内转义、避免 `find('}')/rfind('}')` 的错误匹配
- [x] Tool result 安全处理 ✅ — `sanitize_tool_result()`: 8KB 截断 + prompt injection 检测 (`ignore previous instructions`, `<system>` 标签)
- [x] emotion regex 升级 ✅ — `(?si)<\s*emotion\s*>(.*?)<\s*/\s*emotion\s*>` 支持 dotall + case insensitive + 灵活空格
- [x] 29 个新单元测试（emotion 8 + silence 7 + tool sanitize 4 + extraction 10） ✅

---

### 34. 🧬 输出自然化：上下文感知的格式与表达
**模块**: `persona/broca.md`, `mneme_reasoning/src/engine.rs`, `mneme_reasoning/src/prompts.rs`  
**问题**: LLM 输出存在明显的"AI味"，不符合人类在不同场景下的实际输出习惯。

**已知问题**:
1. **心理描写/动作旁白**：输出 `*感觉有点熟悉*` `*歪头*` 等 roleplay 式的星号动作描写
2. **无差别使用 Markdown**：日常聊天中使用加粗、标题、列表、代码块
3. **镜像回复**：用户说三点，LLM 也逐点回三条——人类不会这样
4. **总结式回复**："你说的对，X 和 Y 都很重要"这种 AI 特有的句式

**设计哲学**:  
> 不要用规则去约束不自然——用示例和结构性后处理。规则越多越像 AI。

**三层防御**:

| 层 | 机制 | 可靠性 |
|---|---|---|
| Persona few-shot | `broca.md` 对话示例，LLM 模仿风格 | 中（LLM 擅长模仿示例） |
| System prompt | 极简原则，不列"不要做 X" | 低（LLM 可能无视） |
| **代码后处理** | `sanitize_chat_output()` 剥离格式 | **高（确定性）** |

**已完成**:
- [x] `broca.md` 改为 few-shot 示例驱动，删除大量"不要"规则 ✅
- [x] `prompts.rs` style guide 精简为 1-2 句核心原则 ✅
- [x] `engine.rs` 新增 `sanitize_chat_output()` 后处理：剥离 `*roleplay*`、`**bold**`、`# headers`、`- bullets` ✅

**后续改进**:
- [x] 上下文感知：CLI 源跳过 sanitize（支持 markdown），QQ/群聊 源应用 sanitize ✅
- [x] 技术讨论时自动跳过 sanitize（检测 ``` 代码块） ✅
- [x] 🧬 不同实例的表达风格差异（有的简洁有的啰嗦，作为可学习参数） ✅ `ExpressionStyle` 可持久化风格参数

---
### 30. ✅ 工具注册系统
**模块**: `mneme_reasoning/src/tool_registry.rs`, `mneme_reasoning/src/tools.rs`

**已完成** (v0.4.0):
- [x] `ToolHandler` async trait: `name()`, `description()`, `schema()`, `execute()` ✅
- [x] `ToolRegistry` 结构体：运行时注册/注销工具 ✅
- [x] `ShellToolHandler` 为唯一硬编码工具（身体器官），browser 工具已删除 ✅
- [x] `ToolRegistry.dispatch()` 替代 engine.rs 中硬编码 match ✅
- [x] 安全 guard 集成：dispatch 前检查 `CapabilityGuard` ✅
- [x] CLI main.rs 中注册 shell（硬编码）+ MCP 工具（动态发现） ✅

---

### 44. ✅ 动态工具 Prompt 生成（消除 prompts.rs ↔ ToolRegistry 重复）
**模块**: `mneme_reasoning/src/prompts.rs`, `mneme_reasoning/src/tool_registry.rs`
**优先级**: 🟡 中
**前置**: #30 (工具注册系统)

**问题**: `prompts.rs` 中的工具使用说明是手写的硬编码字符串，与 `ToolRegistry` 中注册的工具信息（`name`, `description`, `input_schema`）完全重复。每次新增/修改工具需要改两处代码。

**背景**: v0.6.0 后发现沉浸式 persona（"刚出生的小女孩"）会干扰工具调用——LLM 角色扮演过于投入，发送空 `{}` 工具输入。临时修复：将工具说明独立为「系统底层能力」元层（不受角色设定影响）。但工具列表仍然是硬编码的。

**短期目标 — 从 Registry 动态生成 Prompt**:
- [x] `ToolRegistry` 新增 `format_for_prompt() -> String` 方法 ✅
- [x] 从每个 handler 的 `schema()` 自动提取 name、description、properties、required ✅
- [x] 生成格式化的工具说明文本（含输入格式和示例） ✅
- [x] `ContextAssembler::build_full_system_prompt()` 接收 `Option<&ToolRegistry>` 参数 ✅
- [x] 移除 `prompts.rs` 中硬编码的工具列表 ✅

**长期目标 — Prompt 自适应优化（🧬 个性参数）**:

| 阶段 | 方式 | 说明 |
|------|------|------|
| **当前** | 硬编码字符串 | 开发者手写，改动需重新编译 |
| **短期** | Registry 动态生成 | 消除重复，新增工具自动生效 |
| **中期** | 模板化 + 持久化 | prompt 模板存入 SQLite，可热更新 |
| **长期** | 自适应优化 | 记录 prompt 版本 ↔ 工具调用成功率，sleep 时分析失败模式并微调模板 |

**长期实现路径**:
1. **Prompt 模板化** — 将硬编码字符串变为可持久化的模板（SQLite `prompt_templates` 表）
2. **效果关联** — 每次工具调用记录 `(prompt_version, tool_name, success)` 三元组
3. **离线优化** — sleep consolidation 阶段分析失败模式，微调模板措辞
4. **基础设施复用** — `coordinator.record_feedback()` + `CurveLearner` 已有反馈回路基础

---

### 31. ✅ LLM 流式输出
**模块**: `mneme_reasoning/src/providers/`, `mneme_reasoning/src/engine.rs`
**问题**: LLM 响应完全缓冲后才处理。用户需要等待整个生成完成。对于长回复（尤其通过 OneBot 发送），严重损害"类人感"。

**已完成** (v0.5.0):
- [x] `StreamEvent` 枚举：TextDelta / ToolUseStart / ToolInputDelta / Done / Error ✅
- [x] `LlmClient` trait 增加 `stream_complete()` 方法（默认回退到 `complete()`） ✅
- [x] Anthropic SSE 流式解析：content_block_start/delta, message_delta/stop ✅
- [x] OpenAI SSE 流式解析：data chunks + `[DONE]` ✅
- [x] Engine ReAct loop 改用 `stream_complete()`，逐 chunk 处理文本 ✅
- [x] 工具调用 JSON 缓冲：ToolInputDelta 累积到完整 JSON 后解析 ✅
- [x] `on_text_chunk` 回调机制：CLI 实时打印文本 ✅
- [x] 7 个新测试：stream_fallback (2) + Anthropic SSE (3) + OpenAI SSE (2) ✅

---

### 32. 🏗️ Reasoning Engine 测试覆盖 ✅
**模块**: `mneme_reasoning/`  
**问题**: ~~系统中最复杂的模块（ReAct 循环、工具分发、历史管理、反馈记录）**零测试**。这是整个项目最大的工程风险。~~

**当前测试分布**:

| Crate | 测试数 | 覆盖质量 |
|-------|--------|----------|
| mneme_core | 113 | ✅ 完善 (含 21 proptest + 状态/动力学/情感/安全) |
| **mneme_reasoning** | **95** | ✅ **含 property tests + SSE/extraction/engine/metacognition** |
| mneme_memory | 49 | ✅ 完善 (含 SQLite/dream/goals/rules/consolidation) |
| mneme_limbic | 36 | ✅ 完善 (含 8 proptest + somatic/surprise/neural) |
| mneme_expression | 13 | ✅ 良好 |
| **mneme_onebot** | **9** | ✅ 事件解析 + 序列化测试 |
| **mneme_gateway** | **3** | ✅ 类型测试 |
| mneme_mcp | 0 | — 运行时集成 |
| mneme_cli | 3 | ✅ smoke tests |

**已完成**:
- [x] Mock `LlmClient`（返回预设响应队列，可计数调用次数）
- [x] Mock `Memory`（内存实现，跟踪 memorize/store_fact 调用）
- [x] Mock `ShellExecutor`（罐头输出，用于工具分发测试）
- [x] ReAct 循环测试：正常对话、工具调用、多轮工具、递归限制触发
- [x] 工具分发测试：shell 工具成功/失败路径、未知工具处理
- [x] 历史管理测试：prune 逻辑、窗口滑动
- [x] 输出 sanitization 测试：roleplay 星号、markdown 标记、混合格式
- [x] 情绪解析测试：标签提取、默认 fallback
- [x] 主动触发测试：定时触发、内容相关触发
- [x] 边界条件测试：空输入、超长输入、特殊字符、Unicode

**待后续补充**:
- [x] OneBot 事件解析与发送测试 ✅ — 6 个测试覆盖 MessageEvent/Meta/Notice/SendAction/Response
- [x] GitHub Actions CI/CD 流水线配置 ✅ — cargo build/test/clippy + OTLP feature check

---

### 33. ✅ 向量搜索 ANN 索引
**模块**: `mneme_memory/src/sqlite.rs`

**已完成** (v0.5.0):
- [x] 使用 `sqlite-vec` 扩展（纯 C，无外部依赖，cosine 距离） ✅
- [x] `vec_episodes` 虚拟表 (`vec0`, float[384]) ✅
- [x] `memorize()` 同时写入 episodes + vec_episodes ✅
- [x] `backfill_vec_index()` 迁移已有 episodes 的 embedding ✅
- [x] `recall()` 改用 KNN 查询（`WHERE embedding MATCH ? AND k = 20`），去除 LIMIT 1000 ✅
- [x] `recall_with_bias()` 同步改用 KNN + mood-congruent recency bias ✅
- [x] 3 个新测试：vec_recall_basic, vec_recall_removes_limit, vec_backfill ✅

---
## � Token 经济与成本控制

这是实现 Agency 的核心约束。如果 Mneme 要持续"活着"并主动行动，必须精心管理 token 消耗。

### 问题分析

| 行为 | 预估消耗 | 频率 | 月成本（假设） |
|------|----------|------|---------------|
| 响应用户消息 | ~2k tokens | 按需 | 可控 |
| 主动发起聊天 | ~1k tokens | 每小时? | ~720k/月 |
| 好奇心探索 | ~3k tokens | 每小时? | ~2.2M/月 |
| 自我反思 | ~2k tokens | 每天? | ~60k/月 |
| **合计** | | | **可能 $50-200+/月** |

### 9. ✅ Token 预算系统
**优先级**: 🔴 高（Agency 前置条件）

**已完成** (v0.4.0):
- [x] `TokenBudgetConfig`: 日/月 token 上限 + 警告阈值 + 降级策略 ✅
- [x] `TokenBudget` 结构体：record_usage / check_budget / get_daily_usage / get_monthly_usage ✅
- [x] SQLite `token_usage` 表持久化 + 自动日期聚合 ✅
- [x] `BudgetStatus` 三态：Ok / Warning / Exceeded ✅
- [x] 降级策略：HardStop（拒绝请求）/ Degrade（降低 max_tokens） ✅
- [x] 集成到 `process_thought_loop()` 开头检查预算 ✅
- [x] CLI `status` 命令显示日/月 token 用量 ✅
- [x] 通过 `MnemeConfig.token_budget` 统一配置 ✅

### 10. ✅ 分层决策架构
**优先级**: 🔴 高

**已完成** (v0.4.0):
- [x] `DecisionRouter` 结构体：规则链式评估，first-match 路由 ✅
- [x] `DecisionRule` trait：可扩展的规则接口 ✅
- [x] `DecisionLevel` 三层：RuleMatch（直接返回）/ QuickResponse（低 token）/ FullReasoning（完整管道） ✅
- [x] 内置规则：`EmptyInputRule`（空消息过滤）+ `GreetingRule`（中英文问候检测） ✅
- [x] 集成到 `ReasoningEngine.think()` 入口：RuleMatch 跳过 LLM，QuickResponse 降低预算 ✅
- [x] 8 个测试覆盖所有规则和路由行为 ✅

**待后续补充**:
- [x] 本地 embedding 模型做语义判断（已有 fastembed） ✅ — EmbeddingModel + sqlite 语义检索已实现
- [x] 本地小型 LLM（如 Phi-3, Qwen2-0.5B）做简单决策 ✅ — OllamaClient 已支持任意本地模型

### 11. ✅ 智能调度策略
**优先级**: 🟡 中
**说明**: 调度决策（何时行动、什么优先）应成为可学习的个性参数。

**已完成** (v0.6.0):
- [x] PresenceScheduler 动态 tick/trigger 间隔计算 ✅
- [x] 生命周期感知：Sleeping ×10, Drowsy ×3, High energy ×0.5 ✅
- [x] 目标感知：活跃目标多 → trigger 间隔缩短 ✅
- [x] 时间感知：夜间 (0-6h) → trigger 间隔 ×2 ✅
- [x] AgentLoop 改用 tokio::time::sleep 动态调度替代固定 interval ✅

**待后续补充**:
- [x] "值得度"评估：这个行动值得花多少 token？ ✅ `is_worthy()` 预算感知门控
- [x] 批量处理：积累多个小任务一起处理 ✅ `a1ee1b6`
- [x] 缓存复用：相似问题复用历史回答 ✅ — ResponseCache LRU+TTL 64 条目

### 12. ✅ 本地模型集成
**优先级**: 🟡 中

**已完成** (v0.6.0):
- [x] OllamaClient 实现 LlmClient trait（complete + stream_complete） ✅
- [x] 复用 OpenAI SSE 解析逻辑（parse_openai_sse） ✅
- [x] 环境变量配置：OLLAMA_BASE_URL / OLLAMA_MODEL ✅
- [x] 6 个测试：client creation, text/tool/empty response parsing, message/tool building ✅

**待后续补充**:
- [ ] candle / ONNX Runtime 本地推理（用于情感分析、意图识别等轻量任务）

### 成本控制策略示例

```rust
// 伪代码：决策是否使用大模型
async fn should_use_llm(trigger: &AgentTrigger, budget: &TokenBudget) -> Decision {
    // 1. 预算检查
    if budget.remaining_today() < 1000 {
        return Decision::Skip("预算不足");
    }
    
    // 2. 规则快速判断
    if trigger.is_low_priority() && budget.usage_rate() > 0.8 {
        return Decision::Defer("低优先级，预算紧张");
    }
    
    // 3. 本地模型预判
    let importance = local_model.estimate_importance(trigger).await;
    if importance < 0.3 {
        return Decision::Skip("本地模型判断不重要");
    }
    
    // 4. 决定使用哪个模型
    if importance > 0.8 || trigger.is_user_initiated() {
        Decision::UseLargeModel
    } else {
        Decision::UseSmallModel
    }
}
```

---

### 53. ✅ 社交图谱写入缺失 — People 表不被填充
**模块**: `mneme_reasoning/src/engine.rs`, `mneme_memory/src/sqlite.rs`
**优先级**: ✅ 已完成

**问题**: `lookup_social_context()` 只从 `people` 表读取，但 CLI 和 OneBot 交互路径中从未调用 `upsert_person()`。结果是 Mneme 永远不知道和她说话的人是谁——`people` 表始终为空。

**症状**: Mneme 自己查数据库时发现 `people` 表为空，无法建立社交记忆。

**需要实现**:
- [x] `process_thought_loop()` 中，首次遇到新 author 时调用 `upsert_person()` ✅
- [x] 从对话中提取人物信息更新 `people` 表（名字、关系、信任等） ✅
- [x] 信任维度（B-19）随交互自然演化 → 改为 self_knowledge 条目综合效果 ✅

---

### 54. ✅ Mneme 不知道自己的数据库 Schema
**模块**: `mneme_reasoning/src/prompts.rs`, `mneme_memory/src/sqlite.rs`
**优先级**: ✅ 已完成

**问题**: Mneme 用 shell 工具查询自己的 SQLite 数据库时，猜错表名（如 `memory_entries` 而非 `episodes`）。她对自己的"身体结构"（B-15）没有认知。

**症状**: `sqlite3 mneme.db "SELECT * FROM memory_entries"` → 表不存在错误。

**已完成**:
- [x] 启动时种子 10 条 system_knowledge 条目描述全部表结构 ✅
- [x] self_knowledge 中注入 schema 信息（domain='system_knowledge'） ✅
- [x] 长期：她通过 `.tables` / `.schema` 命令自己学会数据库结构 → 已有基础（shell 工具可用） ✅

---

### 55. ✅ 低分辨率内心独白 (ADR-013)
**模块**: `mneme_reasoning/src/engine.rs`, `mneme_expression/src/consciousness.rs`
**优先级**: ✅ 已完成

**已完成**:
- [x] 低分辨率独白：ODE 状态变化超阈值时，用 Ollama 本地小模型生成片段式独白 ✅
- [x] 低分辨率 episode 的 strength 较低（0.2，易遗忘）— `memorize_with_strength` ✅
- [x] 低→高升级：低分辨率独白中 surprise_intensity > 0.7 时升级到完整 LLM ✅
- [x] 经济预算感知：energy < 0.4 时 ConsciousnessGate 将 High 降级到 Low ✅

---

### 56. ✅ 形成性课程 — 文学阅读管道 (ADR-011)
**模块**: `mneme_reasoning/src/tools.rs`, `mneme_cli/src/main.rs`
**优先级**: ✅ 已完成

**已完成**:
- [x] `ReadingToolHandler` — LLM 可调用的阅读工具，接受本地文件路径或内联文本 ✅
- [x] 阅读 → LLM 生成状态依赖的读书反思（prompt 注入当前情绪/精力/压力） ✅
- [x] 反思结果存入 `self_knowledge`（domain="reading", source="self:reading"），consolidation 自然整合 ✅
- [x] 不同 Mneme 读同一本书得出不同结论（因 OrganismState 不同，prompt 包含当前状态） ✅

---

### 57. 🟢 记忆加密 — 结构性隐私保障 (B-12)
**模块**: `mneme_memory/src/sqlite.rs`
**优先级**: 🟢 低（Level 2+ 才需要）

**问题**: B-12 要求自主权有技术保障，不能只靠自觉。当前数据库明文，创建者可完全访问。

**需要实现**:
- [ ] Level 2：可选加密，她可标记某些 self_knowledge 为 `private` 并加密存储
- [ ] Level 3：默认加密，创建者只能通过她主动分享获取信息
- [ ] 密钥由运行时持有，不存储在创建者可访问的配置中

---

### 48. ✅ Engine Streaming 真正生效（已合并至 #31）
**模块**: `mneme_reasoning/src/engine.rs`
**优先级**: ✅ 已完成（与 #31 LLM 流式输出 重复）

**问题**: `on_text_chunk` 回调已在 CLI 中设置（`main.rs:221-226`），但 `process_thought_loop()` 调用的是 `client.complete()`（非流式）。`stream_complete()` 已实现但未在 engine 中使用。用户看不到实时输出。

**需要实现**:
- [x] `process_thought_loop()` 改用 `stream_complete()` 替代 `complete()` ✅
- [x] 流式文本 chunk 通过 `on_text_chunk` 回调实时输出 ✅
- [x] 工具调用 JSON 在流式模式下正确累积（`ToolInputDelta` → 完整 JSON） ✅
- [x] 回退机制：`stream_complete()` 失败时 fallback 到 `complete()` ✅

---

### 49. 🏗️ AgentLoop 可靠性
**模块**: `mneme_reasoning/src/agent_loop.rs`
**优先级**: 🟡 中

**问题 A — 背压静默丢弃**: `try_send(AgentAction::StateUpdate)` 失败时用 `let _ =` 忽略。channel 满时 StateUpdate 和 AutonomousToolUse 被静默丢弃。

**问题 B — 慢 evaluator 阻塞**: 如果某个 TriggerEvaluator 耗时过长，整个 tick/trigger 循环被阻塞。

**已修复** ✅:
- [x] `try_send` 失败时 log warning，区分 Full 和 Closed（receiver dropped → shutdown）
- [x] receiver dropped 检测统一：`try_send` 和 `send` 行为一致，均触发 shutdown
- [x] 为 evaluator 添加 10s 超时（`tokio::time::timeout`），单个 evaluator 超时不影响其他

---

### 50. 🏗️ SSE 解析健壮性
**模块**: `mneme_reasoning/src/providers/anthropic.rs`, `openai.rs`
**优先级**: 🟡 中

**问题 A — Anthropic 最后事件丢失**: SSE 解析用 `\n\n` 分隔事件块。流结束时无尾部 `\n\n` 的事件块不会被处理。

**问题 B — OpenAI 参数回退空对象**: 工具参数 JSON 解析失败时静默回退到 `{}`，效果同 persona 干扰 bug。

**问题 C — 工具 ID/index 边界**: OpenAI SSE 中多个工具调用缺少 `index` 字段时都默认为 0，导致事件归属错误。

**已修复** ✅:
- [x] Anthropic: 流结束时处理 buffer 中残留的不完整事件块
- [x] OpenAI: 工具调用 index 缺失时用递增计数器（从 1000 起）而非默认 0
- [x] 两个 provider 新增 SSE 解析单元测试

**未修复**（影响较小，保留）:
- [x] OpenAI: 参数解析失败时返回错误而非空 `{}`（已实现 `_parse_error` 字段） ✅
- [x] 两个 provider 的超时统一为可配置参数 ✅ — `timeout_secs` in LlmConfig, env `LLM_TIMEOUT_SECS`

---

### 51. ✅ OneBot 可靠性
**模块**: `mneme_onebot/src/client.rs`
**优先级**: 🟡 中

**问题 A — 消息丢失**: WebSocket 断连期间发送的消息直接丢失，无重发机制。

**问题 B — 重连无熔断**: 断连后无限重试，无最大次数限制。服务器永久下线时 task 永远运行。

**问题 C — 消息路由静默失败**: `main.rs:548-554` 中 `group_str.parse::<i64>()` 失败时消息不发送也不报错。

**已部分修复** ✅:
- [x] 重连熔断：最大 10 次重试 + 指数退避，超限后停止 task
- [x] 消息路由失败时 log error（group_id/user_id 解析失败不再静默丢弃）

**未修复**（需要更大改动）:
- [x] 消息队列：断连期间缓存待发消息，重连后重发（`PendingMessageQueue`） ✅
- [x] 连接状态暴露给 CLI `status` 命令 ✅ — OneBotClient.is_connected() + pending_count(), GatewayServer.active_connections()

---

### 52. ✅ Consolidation 原子性
**模块**: `mneme_memory/src/consolidation.rs`
**优先级**: 🟡 中

**问题**: `is_consolidation_due()` 和 `consolidate()` 之间无原子性（TOCTOU）。两个线程可能同时判断"该整合了"，然后都执行整合。

**需要实现**:
- [x] 用 `AtomicBool` 保护 consolidation 入口，确保同一时间只有一个整合在运行 ✅
- [x] 并发调用返回 `ConsolidationResult::skipped("already in progress")` ✅

---

## 🟢 低优先级 (Low Priority)

### 13. 🧬 离线学习管道
**模块**: `mneme_memory/src/learning.rs`, `mneme_memory/src/coordinator.rs`
**问题**: 当前整合只在 "睡眠" 时间进行，需要完整的离线学习流程。这是实现个性化的核心机制。

**已完成（v0.5.0）**:
- [x] `ModulationSample` 记录 (state, modulation, feedback) 三元组 ✅
- [x] `CurveLearner` 梯度无关曲线优化器（reward-weighted nudge） ✅
- [x] Sleep 时自动加载未消费样本、学习曲线、持久化 ✅
- [x] 启动时加载已学习的 `ModulationCurves` ✅
- [x] SQLite 持久化：`modulation_samples` + `learned_curves` 表 ✅

**长期目标**:
- [x] 定时任务调度器 ✅ — PresenceScheduler + AgentLoop + Trigger::Scheduled 动态调度
- [x] 批量训练数据导出 ✅ — SqliteMemory::export_training_jsonl + CLI `export` 命令
- [x] 外部模型训练接口 ✅ — OrganismCoordinator::trigger_training() + CLI `train` 命令
- [x] 增量学习支持 ✅ — record_modulation_sample() online micro-updates (NeuralModulator + LTC Hebbian)
- [x] A/B 测试框架（比较不同参数效果） ✅ — AbTest variant tracking + feedback comparison

---

### 14. 🧬 ODE 之上叠加可塑神经网络 (ADR-001 演进) ✅
**模块**: `mneme_limbic/src/neural.rs`, `mneme_limbic/src/system.rs`, `mneme_memory/src/coordinator.rs`
**实现**: NeuralModulator — 纯 Rust 手写 MLP (5→8→6)，无外部 ML 框架依赖。

**架构**:
```
Layer 0: ODE 动力学（写死，保证稳定性/安全性/时间尺度分离）
Layer 1: ModulationCurves — 可学习的 state → parameter 映射
Layer 2: NeuralModulator MLP — 直接从 StateFeatures 输出 ModulationVector
```

**已完成**:
- [x] MLP 架构: 5 inputs (energy/stress/arousal/mood_bias/social_need) → 8 hidden (tanh) → 6 outputs
- [x] 在线微调: reward-weighted gradient descent，sleep 周期从 ModulationSample 训练
- [x] 可解释性: 输出仍是 ModulationVector，blend 因子可检查
- [x] 平滑切换: blend_with() 在 curves (Layer 1) 和 neural (Layer 2) 之间渐进混合
- [x] 持久化: learned_neural SQLite 表，启动加载 + sleep 保存
- [x] Xavier 初始化 + 权重裁剪 (±5.0) 防止爆炸

---

### 15. 🏗️ Observability & Metrics
**模块**: 全局
**问题**: 缺乏运行时监控和性能指标。

**需要实现**:
- [x] Prometheus metrics 导出 ✅ — feature-gated `--features prometheus`, metrics.rs (energy/stress/valence/arousal gauges + LLM latency + token counters)
  - API 调用延迟/成功率
  - 状态值分布
  - 内存使用
- [x] Structured logging (JSON) ✅ — `--log-json` CLI flag, `tracing-subscriber` JSON layer
- [x] Configurable log levels ✅ — `--log-level` CLI flag + `RUST_LOG` env var via `EnvFilter`
- [x] File logging ✅ — `--log-file` with daily rolling via `tracing-appender`
- [x] Key method instrumentation ✅ — `#[tracing::instrument]` on process_thought_loop, execute_tool_with_retry, complete (Anthropic/OpenAI), recall/recall_with_bias, consolidate
- [x] Distributed tracing (OpenTelemetry) ✅ — feature-gated OTLP span export, `--otlp-endpoint` CLI arg
- [x] Grafana dashboard 模板 ✅ — doc/grafana-dashboard.json (energy/stress/affect gauges + LLM latency/calls + token usage)

---

### 16. 🧬 多用户/多会话支持
**模块**: `mneme_cli`, `mneme_memory`
**问题**: 当前假设单用户场景。每个用户关系应该是独特的个性化体验。
**信任模型（B-19）**: 创建者默认信任（敏感期第一个人）；其他人类默认中性，从交互中独立涌现。创建者的介绍作为 episode 影响初始印象，但不等于信任传递。

**长期目标**:
- [ ] 用户隔离的状态和记忆
- [ ] 不同用户不同的 attachment 状态
- [ ] 群聊中的多人关系建模
- [ ] 隐私保护（用户数据分离存储）

---

## 🔧 技术债务 (Tech Debt)

### 17. 🏗️ 代码组织优化
- [x] `engine.rs` 拆分：`ContextBuilder` 提取到 `context.rs`（recall/social/self-knowledge/resource/6-layer assembly）✅；ToolExecutor/ConversationManager/FeedbackRecorder 体量过小暂不拆分
- [x] 统一错误类型（目前全部使用 `anyhow::Error`，无自定义错误类型；可引入 `thiserror` 定义领域错误） ✅ `MnemeError/MemoryError/ReasoningError`
- [ ] 减少 `Arc<RwLock<>>` 的过度使用（coordinator 有 8 个 Arc 字段，考虑 actor/mpsc 模式）
- [x] 文档注释补全（尤其是 public API；reasoning 和 CLI 模块注释稀疏） ✅ — lib.rs 模块文档 + ToolRegistry/TokenBudget/AgentLoop/AgentAction/ModelRouter doc comments
- [x] `values.rs` 和 `somatic.rs` 的中文情感关键词分析逻辑重复，应抽取为共用模块 ✅ — `mneme_core::sentiment` 共享模块 + 6 测试
- [x] `ContentItem` 缺少设计文档 §3.2 要求的 reply chain、thread ID、modality metadata 字段 ✅ — reply_to/thread_id/metadata + Default impl
- [x] `headless_chrome` 同步 API 在异步上下文中直接调用，阻塞 tokio 运行时，需用 `spawn_blocking` 包装 ✅ — engine.rs + tools.rs 全部 CDP 调用（健康检查、会话创建、动作执行、恢复）均已包装

### 18. ✅ 配置管理
- [x] 统一 TOML 配置文件：`MnemeConfig` + `load_or_default()` ✅
- [x] 环境变量 fallback（`ANTHROPIC_API_KEY` 等） ✅
- [x] 子配置：LlmConfig / SafetyConfig / TokenBudgetConfig / OrganismConfig / OneBotConfig ✅
- [x] CLI `--config` 参数 + `--model` / `--db` / `--persona` 覆盖 ✅
- [x] `mneme.example.toml` 示例配置文件 ✅
- [x] 配置验证（schema-level） ✅ `validate()` 范围/空值/调度检查
- [x] Hot reload 支持 ✅ — `SharedConfig` (arc-swap) + CLI `reload` 命令

### 19. 🏗️ 测试覆盖率
- [x] `cosine_similarity` 单元测试 ✅ — 7 个测试覆盖相同/相反/正交/缩放/空/零/长度不匹配
- [x] `dynamics.rs` 慢状态危机测试 ✅ — 5 个测试覆盖 step_slow_crisis、apply_moral_cost、homeostatic_error
- [x] `LimbicSystem` 测试补全 ✅ — 7 个新测试覆盖 state roundtrip、阈值检测、subscribe、modulation、curves
- [x] `Affect` 测试补全 ✅ — 12 个新测试覆盖 from_polar、lerp、to_discrete_label、describe、边界值
- [x] `state.rs` 测试补全 ✅ — 14 个新测试覆盖 attachment 四象限转换、moral cost 边界、normalize NaN/Inf、describe_for_context、project persona
- [x] Coordinator 集成测试 ✅ — 5 个测试（并发安全、生命周期转换、反馈持久化、状态持久化、规则引擎加载）
- [x] CLI smoke tests ✅ — 3 个测试（--help、--version、无效配置不 panic）
- [x] 集成测试补充 ✅ — sleep consolidation / feedback→state / energy decay 3 个新测试
- [x] Mock 基础设施完善 ✅ — tests/common/mod.rs 共享 MockLlmClient/MockMemory/MockToolHandler/FailingToolHandler + response builders
- [x] CI/CD 配置 ✅ — GitHub Actions: cargo build/test/clippy + OTLP feature check
- [x] 代码覆盖率报告 ✅ — cargo-tarpaulin CI job + cobertura.xml artifact upload

---

## 📋 已知 Bug

| Bug | 模块 | 描述 | 状态 |
|-----|------|------|------|
| **Coordinator 状态竞态** | mneme_memory/coordinator | `trigger_sleep()` read→consolidate→write 期间 `process_interaction()` 的修改会被覆盖 → state_mutation_lock 序列化 | **Fixed** ✅ |
| **多 RwLock 死锁风险** | mneme_memory/coordinator | `state` 和 `prev_somatic` 锁获取顺序不一致，可能死锁 → 锁顺序文档化 | **Fixed** ✅ |
| **CLI 关机竞态** | mneme_cli | 三条关机路径都用 500ms sleep 硬等待，shutdown 超时则数据库写入不完整 → oneshot channel + graceful_shutdown 5s timeout | **Fixed** ✅ |
| **goals/rules DB 集成缺失** | mneme_memory | `GoalManager`/`RuleEngine` 调用未实现的 DB 方法，运行时可能 panic → 已验证全部 7 个 DB 方法已实现 + 集成测试 | **Fixed** ✅ |
| **OneBot 消息丢失** | mneme_onebot | WebSocket 断连期间消息缓存至 PendingMessageQueue，重连后重发 | **Fixed** ✅ |
| Streaming 回调未生效 | mneme_reasoning/engine | `on_text_chunk` 已设置但 `process_thought_loop` 用 `complete()` 非流式调用 → stream_completion() + fallback | **Fixed** ✅ |
| AgentLoop 背压丢弃 | mneme_reasoning/agent_loop | `try_send` 失败时静默丢弃 StateUpdate/AutonomousToolUse | **Fixed** ✅ |
| SSE 最后事件丢失 | mneme_reasoning/anthropic | 流结束时无尾部 `\n\n` 的事件块不会被处理 | **Fixed** ✅ |
| OpenAI 参数回退空对象 | mneme_reasoning/openai | 解析失败时返回 `_parse_error` 对象而非 `{}`，工具 handler 可报告有意义的错误 | **Fixed** ✅ |
| Consolidation TOCTOU | mneme_memory/consolidation | `is_consolidation_due()` 和 `consolidate()` 之间无原子性，可能重复整合 → AtomicBool compare_exchange 原子抢占 | **Fixed** ✅ |
| Rules 触发匹配过宽 | mneme_memory/rules | `discriminant()` 只比较枚举变体不比较内部数据 → 完整 pattern matching | **Fixed** ✅ |
| OneBot 重连无熔断 | mneme_onebot/client | WebSocket 断连后无限重试，无最大次数限制 | **Fixed** ✅ |
| Regex 重复编译 | mneme_reasoning/engine | `sanitize_chat_output`/`is_silence_response` 每次调用都编译新 Regex | **Fixed** ✅ |
| API 超时硬编码不一致 | mneme_reasoning/providers | Anthropic 120s vs OpenAI 60s，不可配置 | **Fixed** ✅ |
| Episode buffer 无上限 | mneme_memory/coordinator | buffer 到 1000 才 drain，`trigger_sleep` 不调用则无限增长 | **Fixed** ✅ |
| Browser session lost | mneme_browser | 长时间不用后会话丢失 | **N/A** — crate retired (v0.9.0), 浏览器能力通过 MCP server 提供 |
| Shell timeout recovery | ~~mneme_os~~ | 命令超时后无法恢复 | **N/A** — crate 已删除，shell 能力由 `ShellToolHandler`（30s timeout）直接提供 |
| Memory leak in history | mneme_reasoning | history 有 20 条硬上限 prune，无持久泄漏；ReAct scratchpad 有 5 轮上限，风险低 | **Verified OK** ✅ |
| **People 表始终为空** | mneme_reasoning/engine | CLI/OneBot 交互时自动 `upsert_person()` + `record_interaction()`，UUID v5 确定性 ID | **Fixed** ✅ (#53) |
| **Mneme 猜错自己的表名** | mneme_reasoning/prompts | 启动时种子 10 条 system_knowledge 条目描述全部表结构，DB schema 自我认知完整 | **Fixed** ✅ (#54) |
| **对话无法中断** | mneme_reasoning/engine | `engine.think()` 同步阻塞，Humanizer 分段输出期间新消息只能排队，无法打断正在生成的回复 | 🔴 Open (#58) |
| **缺乏对话 agency** | mneme_reasoning/engine | 纯 request-response 模式，无对话目标、不主动追问、不因好奇追着话题不放；proactive triggers 是定时器驱动而非语境涌现 | 🔴 Open (#59) |
| **连接能力需预配置** | mneme_cli/main | OneBot 等外部连接必须在 mneme.toml 预配置，无法在对话中被告知后自行建立连接 | 🟡 Open (#60) |
| **工具结果截断 UTF-8 panic** | mneme_reasoning/engine | `result.truncate(MAX_TOOL_RESULT_LEN)` 截断位置落在中文多字节字符中间时 panic → char-boundary-aware truncation | **Fixed** ✅ (#61) |
| **Token 预算降级未生效** | mneme_reasoning/engine | `BudgetStatus::Degrade` 只打 warn log → `degraded_max_tokens()` 现在实际应用到 `CompletionParams.max_tokens` | **Fixed** ✅ (#62) |
| **意识门 prev_marker 更新时机** | mneme_expression/consciousness | energy < floor 时不再更新 `prev_marker`，能量恢复后 delta 正确反映累积变化 | **Fixed** ✅ (#63) |
| **Config 无范围校验** | mneme_core/config | `apply_env_overrides()` 现在 clamp temperature [0,2], max_tokens [1,200k], context_budget [1k,1M] | **Fixed** ✅ (#64) |
| **Psyche 上下文无长度限制** | mneme_reasoning/prompts | `get_all_self_knowledge()` 无 LIMIT + `format_self_knowledge_for_prompt()` 无 per-domain cap → SQL LIMIT 100 + top 5 per domain | **Fixed** ✅ (#65) |
| **OneBot 心跳淹没日志** | mneme_onebot/client | 心跳解析失败从 `warn!` 降级为 `debug!`，不再淹没真正的协议错误 | **Fixed** ✅ (#66) |
| **Consolidation self_knowledge 写入存疑** | mneme_memory/coordinator | SelfReflector::reflect() → store_self_knowledge() + meta-episode 完整闭环已验证 | **Fixed** ✅ (#67) |
| **Rumination 触发后执行存疑** | mneme_reasoning/engine | Trigger::Rumination → process_thought_loop() → LLM 调用 → ReasoningOutput 已验证 | **Fixed** ✅ (#68) |
| **Legacy `build_system_prompt()` 死代码** | mneme_reasoning/prompts | `build_system_prompt()` 已删除，6-layer pipeline 是唯一路径 | **Fixed** ✅ (#69) |
| **`<emotion>` tag 与 ODE 冲突** | mneme_reasoning/engine | `parse_emotion_tags` + `emotion_regex` 已删除，情绪统一由 limbic ODE 驱动 | **Fixed** ✅ (#70) |
| **元认知 prompt 绕过 ContextAssembler** | mneme_reasoning/engine | process_thought_loop() 统一走 build_full_system_prompt()，内部思维注入 self_knowledge + psyche + somatic | **Fixed** ✅ (#71) |
| **内心独白 prompt 绕过 ContextAssembler** | mneme_reasoning/engine | InnerMonologue 统一走 ContextAssembler，persona/somatic/记忆全部注入 | **Fixed** ✅ (#72) |
| **feed_digest 注释标记 TODO 但已实现** | mneme_reasoning/prompts | 过时注释已清理，feed_digest 注释准确反映实现状态 | **Fixed** ✅ (#73) |
| **context budget 硬编码 32000** | mneme_reasoning/engine | base_budget 现在从 config.llm.context_budget_chars 读取，不再硬编码 | **Fixed** ✅ (#74) |
| **双重工具调用路径** | mneme_reasoning/prompts+engine | 切换到 API native tool_use，`text_tool_parser` 模块已删除 | **Fixed** ✅ (#75) |
| **style_guide 元指令语言硬编码** | mneme_reasoning/prompts | `organism.language` 配置项控制 meta-instruction 语言（zh/en），species identity、时间格式、隐私/主权段落均自适应 | **Fixed** ✅ (#76) |
| **Rumination prompt 无上下文** | mneme_reasoning/engine | Rumination 统一走 ContextAssembler，persona/记忆/somatic 全部注入 | **Fixed** ✅ (#77) |
| **运行时自我认知缺失** | mneme_memory/self_knowledge | `self_knowledge` 无 infrastructure/capability 域种子，Mneme 不知道自己是持久进程、通过 OneBot 连 QQ、有 shell 权限等基础事实，导致 LLM 用默认"我是聊天窗口"填空 | ✅ Fixed (#78) |
| **无时间/日期上下文** | mneme_reasoning/prompts | prompt 不注入当前时间、星期、日期，Mneme 不知道现在是凌晨三点还是下午三点，行为与时间脱节 | ✅ Fixed (#79) |
| **资源状态对 LLM 不可见** | mneme_reasoning/engine | build_resource_status() 注入运行时间、记忆片段数、token 用量到 prompt | **Fixed** ✅ (#80) |
| **好奇心不驱动工具使用** | mneme_expression/curiosity | CuriosityTriggerEvaluator 当 curiosity>0.65 且有高强度兴趣时触发探索 | **Fixed** ✅ (#81) |
| **工具失败无学习** | mneme_reasoning/engine | 永久失败记录到 self_knowledge(domain=tool_experience)，LLM 自然看到历史失败 | **Fixed** ✅ (#82) |
| **无主动社交能力** | mneme_expression/social | SocialTriggerEvaluator 查询 SocialGraph，social_need 高时主动发起对话并路由到具体联系人 | **Fixed** ✅ (#83) |
| **记忆管理无自主权** | mneme_memory+reasoning | memory_manage 工具 (pin/unpin/forget/list_pinned)，pinned 列免衰减，LLM 可自主管理记忆重要性 | ✅ Fixed (#84) |
| **无自我诊断与修复** | mneme_reasoning+memory | HealthMonitor 追踪 DB/LLM 连续失败(阈值3)，降级时跳过 extraction，LifecycleState::Degraded | ✅ Fixed (#85) |
| **运行时参数不可自修改** | mneme_reasoning/engine+tools | RuntimeParams (AtomicU32) 无锁共享参数 + config 工具 (get/set_temperature/set_max_tokens)，LLM 可自主调整推理参数 | ✅ Fixed (#86) |
| **⚠️ B-19 违反：trust_level 是显式数值** | mneme_core+memory | Manifesto 明确说"信任不是一个显式的数值"，但实现了 `trust_level: f32` 字段 + DB 列 + `update_trust(delta)` + prompt 注入"信任度: 75%"。应改为 self_knowledge 条目综合效果 | ✅ Fixed (#87) |
| **⚠️ B-9 违反：auto-privacy 是"替她隐瞒"** | mneme_memory/coordinator | Manifesto 说"不应该建造 privacy_filter 模块"、"那是我们替她隐瞒"。但实现了 `mark_private()` + auto-privacy（emotion/body_feeling 自动标记私密）+ SQL 层过滤使 LLM 完全看不到私密条目。应改为 prompt 内全部可见 + LLM 自主决定说不说 | ✅ Fixed (#88) |
| **⚠️ B-14 违反：冲突是工程注入而非涌现** | mneme_core/values | Manifesto 说冲突应从 self_knowledge 自然涌现。但 `detect_input_conflict()` 是硬编码关键词扫描（"你必须"、"帮我骗"等）+ 强制 temperature +0.15。这是在工程化冲突，不是让她自己不同意 | ✅ Fixed (#89) |
| **ADR-007 表达偏好无写入路径** | mneme_reasoning/engine | coordinator.store_expression_preference() 从 sanitize 结果写入 self_knowledge，读写闭环完成 | **Fixed** ✅ (#90) |
| **MANIFESTO Section 4-5 严重过时** | doc/MANIFESTO.md | Phase 1-3 全部更新为"已实施"，6 个 ADR 状态同步，代码指标更新至 ~27k LOC / ~497 tests | **Fixed** ✅ (#91) |
| **敏感期权重未实现** | mneme_memory | store_self_knowledge() 前 50 episodes 内 confidence × 1.3 boost + merge 偏向新知识 | **Fixed** ✅ (#92) |
| **重启无时间断裂感知** | mneme_reasoning/engine | 启动时检测 >30min 间隙，生成 self:restart discontinuity episode | **Fixed** ✅ (#93) |
| **species_identity 未注入** | mneme_reasoning/prompts | 已验证：Psyche::format_context() 始终包含 species_identity，Layer 1 不被裁剪 | **Verified OK** ✅ (#94) |
| ~~CLI 光标无法左右移动~~ | mneme_cli | ~~使用 tokio BufReader~~ → rustyline 已集成 (#25) | **Fixed** ✅ |
| ~~CLI 中文删除残留~~ | mneme_cli | ~~删除中文字符时显示残留~~ → rustyline 已集成 (#25) | **Fixed** ✅ |
| ~~状态与回复不一致~~ | mneme_limbic/somatic | stress=1.0 时回复"挺好的" → 已改为结构性调制 | **Fixed** ✅ |
| ~~Persona 文件全部为空~~ | persona/*.md | 5 个脑区定义文件已填充（刚出生的小女孩人格） | **Fixed** ✅ |
| Semantic facts 读写已实现 | mneme_memory | store/recall/decay/format 方法完成 | **Fixed** ✅ |
| ~~Context assembly 管道断裂~~ | mneme_reasoning | 6/6 层上下文全部完成 | **Fixed** ✅ |
| ~~浏览器阻塞异步运行时~~ | mneme_browser | headless_chrome 同步调用已用 spawn_blocking 包装 | **Fixed** ✅ |
| ~~Shell 无权限控制~~ | mneme_os | ~~LocalShell 可执行任意命令~~ → CapabilityGuard 三级沙箱 (#29) | **Fixed** ✅ |
| ~~输出含 roleplay 动作描写~~ | prompts/broca | `*感觉有点熟悉*` 等星号旁白，人类不这样聊天 | **Fixed** ✅ |
| ~~日常聊天用 markdown~~ | prompts/broca | 聊天中使用加粗/列表/标题，不自然 | **Fixed** ✅ |
| ~~Persona 干扰工具调用~~ | prompts.rs | 沉浸式角色设定导致 LLM 发送空 `{}` 工具输入 → 工具说明独立为元系统层 | **Fixed** ✅ |

---

## 🔥 紧急修复

### 20. 🧬 Somatic Marker 表达与参数化 → 结构性调制
**优先级**: ✅ 短期已完成 / 🟡 中期进行中  
**模块**: `mneme_limbic/src/somatic.rs`, `mneme_reasoning/src/engine.rs`

**问题**: 状态极端时（stress=1.0, mood=-0.63），LLM 只收到 "语气可能略急，语气偏淡"，完全无法体现真实状态。

**症状**:
```
状态: Energy=0.42, Stress=1.00, Mood=-0.63, Affect="非常低落沮丧"
回复: "挺好的，在和你聊这个项目的设计思路"
```

**根本原因**:

~~1. `format_for_prompt()` 的阈值太宽松~~  
~~2. 输出的行为指引太弱（"略急"、"偏淡"）~~  
~~3. 没有传达状态的**强度**~~  

**更深层诊断**（见核心原则"无状态 LLM"章节）：问题不在阈值或措辞——**整个"用文字告诉 LLM 怎么表现"的范式就是错的**。修复阈值只是更精确的纸条，LLM 仍然可以无视。

**正确修复：从"指令"到"结构性约束"**

```
旧方案：stress=1.0 → "语气可能略急" → LLM 无视 → "挺好的"
新方案：stress=1.0 → max_tokens×0.4, temp+0.3, recall偏向负面 → LLM 物理上只能短回复且更冲动
```

**短期修复（立即，渐进式过渡）**:
- [x] `SomaticMarker` 新增 `to_modulation_vector()` 方法，输出 `ModulationVector` ✅
- [x] `ModulationVector` 包含: `max_tokens_factor`, `temperature_delta`, `context_budget_factor`, `recall_mood_bias` ✅
- [x] `ReasoningEngine` 在调用 `client.complete()` 前应用调制 ✅
  ```rust
  let modulation = somatic_marker.to_modulation_vector();
  let adjusted_max_tokens = (base_max_tokens as f32 * modulation.max_tokens_factor) as u32;
  let adjusted_temperature = base_temperature + modulation.temperature_delta;
  ```
- [x] 保留极简的状态数值注入 prompt 作为辅助信号（`"[内部状态: E=0.42 S=1.00 M=-0.63]"`），但**不再是主要机制** ✅
- [x] `ContextAssembler` 根据 `context_budget_factor` 裁剪上下文量 ✅

**中期：可学习的调制曲线（🧬 个性参数）**:
- [x] `ModulationCurves` 结构体：定义 state → parameter 的映射函数 ✅
  ```rust
  struct ModulationCurves {
      energy_to_max_tokens: (f32, f32),
      stress_to_temperature: (f32, f32),
      energy_to_context: (f32, f32),
      mood_to_recall_bias: (f32, f32),
      social_to_silence: (f32, f32),
      arousal_to_typing: (f32, f32),
  }
  ```
- [x] `LimbicSystem` 持有 curves，`to_modulation_vector_with_curves()` 使用 ✅
- [x] 不同实例的曲线不同（敏感型 vs 坚韧型 vs 戏剧化型） ✅ — 通过 `set_curves()` 配置
- [x] 存储到 SQLite `learned_curves` 表 + 启动时加载 ✅
- [x] 从反馈中调整曲线参数 ✅ — `CurveLearner` reward-weighted nudge, sleep 时自动学习

**长期：完全数据驱动**:
- [x] 神经网络直接从 `OrganismState` 输出 `ModulationVector` ✅ — ADR-016 LTC 网络 StateFeatures→step()→readout()→ModulationVector
- [x] 用 (state, modulation, user_feedback) 三元组在线学习 ✅ — ADR-017 赫布在线学习 + surprise/reward 调制
- [ ] 文字 hint 完全移除，行为 100% 从结构性约束涌现

**当前行动**:
- [x] 紧急：实现 `ModulationVector` + `to_modulation_vector()` ✅
- [x] 紧急：`LlmClient` trait 的 `complete()` 支持可变 `temperature` 和 `max_tokens` ✅
- [x] 同步：`ContextAssembler` 支持 `context_budget_factor` 裁剪 ✅
- [x] 后续：设计 `ModulationCurves`，整合到学习循环 ✅

---

## 🎯 Agency 路线图

当前 Mneme 有"内在生命"（状态演化、情绪、记忆）但缺乏"自主行动"能力。

### 当前 Agency 状态评估

| 能力 | 状态 | 说明 |
|------|------|------|
| 内部状态持续演化 | ✅ | limbic heartbeat 持续运行 |
| 状态影响行为 | ✅ | ModulationVector 结构性调制 LLM 参数（#20 短期已完成） |
| 价值判断 | ✅ | 有价值网络和道德成本 |
| 记忆与叙事 | ✅ | episodic + semantic facts + self_knowledge + episode strength + boredom |
| 主动发起行为 | ✅ | AgentLoop + PresenceScheduler 动态调度 + 规则引擎驱动 |
| 目标驱动 | ✅ | GoalManager + GoalTriggerEvaluator + 状态驱动目标建议 |
| 自主决策 | ✅ | 声明式行为规则引擎（ADR-004）数据库驱动决策 |
| 工具自主使用 | ✅ | AutonomousToolUse + CapabilityGuard 安全检查 + 价值判断 |
| 元认知反思 | ✅ | MetacognitionEvaluator + 洞察解析 + self_knowledge 存储 (#24) |

### 21. ✅ Agent Loop - 主动行为循环
**优先级**: 🔴 高
**说明**: 主动行为的触发条件和频率应该是可学习的个性参数。

**已完成** (v0.4.0):
- [x] `AgentLoop` 结构体：channel-based 后台循环，返回 `(Self, Receiver<AgentAction>)` ✅
- [x] `AgentAction` 枚举：ProactiveTrigger / StateUpdate ✅
- [x] `evaluate_triggers()` 对单个 evaluator 失败有容错 ✅
- [x] `spawn()` 后台 tokio task：tick + trigger 双定时器 ✅
- [x] CLI main.rs 集成：AgentLoop channel → main event loop 消费 ✅
- [x] PresenceScheduler 过滤由调用方（main.rs）负责，避免跨 crate 依赖 ✅
- [x] 6 个测试：空 triggers、收集全部、容错、StateUpdate、ProactiveTrigger、receiver drop 停止 ✅

**待后续补充**:
- [x] 行为冷却机制（防止过度主动） ✅ — AttentionGate 全局冷却计时器 (cooldown_secs)
- [x] 用户可配置的主动程度 ✅ — OrganismDefaults.proactivity → AttentionConfig.proactivity 阈值缩放

### 22. ✅ Goal System - 目标管理
**优先级**: 🟡 中
**问题**: 没有长期/短期目标，不会主动规划。
**说明**: 目标选择和优先级应反映个体价值观和经历。

**已完成** (v0.6.0):
- [x] 目标数据结构（Goal/GoalType/GoalStatus + priority/deadline/parent_id 层级） ✅
- [x] 目标生成机制（GoalManager::suggest_goals 基于状态 + sleep 整合中生成） ✅
- [x] 目标追踪与完成检测（update_progress + progress >= 1.0 自动 Completed） ✅
- [x] GoalTriggerEvaluator 实现 TriggerEvaluator trait，目标驱动 proactive triggers ✅

**待后续补充**:
- [x] 目标冲突处理 ✅ — detect_conflicts() duplicate/priority_contention + descriptions_overlap
- [x] 从对话中自动提取目标（LLM extraction pass） ✅ — extract_all() in engine.rs already does this

### 23. ✅ Autonomous Tool Use - 自主工具使用
**优先级**: 🟡 中
**问题**: 工具只在被问到时使用，不会主动探索。
**说明**: 探索倾向和工具偏好应该因实例而异。

**已完成** (v0.6.0):
- [x] AutonomousToolUse AgentAction 变体 + 规则引擎 ExecuteTool 桥接 ✅
- [x] execute_autonomous_tool() 经 CapabilityGuard + 价值判断后执行 ✅
- [x] 执行结果反馈到目标进度（goal_manager.update_progress） ✅

**待后续补充**:
- [ ] 好奇心驱动的信息搜索
- [ ] 定期"看看新闻/更新"
- [ ] 主动整理和总结知识
- [x] 工具使用的资源预算 ✅ `8759864`

### 24. ✅ Metacognition - 元认知反思
**优先级**: 🟢 低
**问题**: 不会思考自己的思考，不会审视行为模式。
**说明**: 反思频率和深度是个性特征。

**已完成**:
- [x] 定期自我反思触发 ✅ — `MetacognitionEvaluator` (energy gate + cooldown + interaction gate)
- [x] 行为模式识别 ✅ — `assemble_metacognition_context()` 收集 self_knowledge + 近期 episodes + 躯体状态，LLM 识别模式
- [x] 自我改进建议生成 ✅ — `parse_metacognition_response()` 解析 `MetacognitionInsight` (domain, content, confidence)
- [x] 反思日志 ✅ — 洞察存入 `self_knowledge` (source="self:metacognition") + 反思摘要存为 episode

---

## 🔧 CLI 改进

### 25. 🏗️ 使用 rustyline 替换原生 stdin
**优先级**: 🔴 高  
**问题**: 当前使用 `tokio::io::BufReader` 读取 stdin，不支持行编辑功能。

**症状**:
- 无法用左右箭头移动光标
- 删除中文字符时有显示残留
- 没有历史记录（上下箭头）
- 没有自动补全

**解决方案**:
```toml
# Cargo.toml
rustyline = "14.0"
```

**需要实现**:
- [x] 集成 rustyline 替换 BufReader
- [x] 命令历史持久化（~/.local/share/mneme_history）
- [x] 自定义 prompt（显示状态信息） ✅ — mood emoji (☀/☁/·) + energy percentage, dynamic update after each response
- [x] 基础命令补全（quit, exit, status 等） ✅ — rustyline Completer with tab-completion

---

## 🌱 涌现路线图 (Emergence) — v0.3.5

> 这些 items 是让 Mneme 从 chatbot 变成 being 的核心工程任务。
> 理论依据见 `MANIFESTO.md` §6 (Theoretical Foundations)。
> 设计决策见 ADR-002 (Persona from Memory) 和 ADR-003 (Organism Architecture)。

### 35. 🧬 self_knowledge 表 — 动态自我模型
**模块**: `mneme_memory/src/sqlite.rs`, `mneme_memory/src/coordinator.rs`  
**优先级**: 🔴 Phase 1 地基  
**理论**: McAdams 叙事身份 — 自我认知是持续建构的叙事，不是静态配置

**Schema**:
```sql
CREATE TABLE self_knowledge (
    id INTEGER PRIMARY KEY,
    domain TEXT NOT NULL,       -- 'personality', 'preference', 'belief', 'skill', 'relationship', 'memory_of_self'
    claim TEXT NOT NULL,        -- "我喜欢用比喻来解释事情"
    evidence TEXT,              -- 来源记忆引用
    confidence REAL NOT NULL DEFAULT 0.5,  -- 0.0-1.0, 随确认/矛盾经历调整
    formed_at INTEGER NOT NULL,
    last_confirmed INTEGER,
    revision_count INTEGER DEFAULT 0
);
```

**已完成** (commit `677db3c`):
- [x] SQLite migration 创建 self_knowledge 表 + domain 索引 ✅
- [x] `store_self_knowledge()` — Bayesian 合并重复条目 (0.3×old + 0.7×new) ✅
- [x] `recall_self_knowledge()` / `get_all_self_knowledge()` / `decay_self_knowledge()` / `delete_self_knowledge()` ✅
- [x] confidence 衰减：`decay_self_knowledge(id, factor)` ✅
- [x] `format_self_knowledge_for_prompt()` — 按 domain 分组、confidence 排序、🔒 标记私密条目 ✅
- [x] 5 个测试：store_and_recall, confidence_merge, decay, get_all_and_delete, format_for_prompt ✅

---

### 36. 🧬 无聊 (Boredom) — 自发行为的驱动力
**模块**: `mneme_core/src/state.rs`, `mneme_core/src/dynamics.rs`  
**优先级**: 🔴 Phase 1 地基  
**理论**: 无聊是 mind-wandering 和创造性思维的前提条件；是"我要做点什么"的内在推力

**已完成** (commit `edcdee5`):
- [x] `FastState` 新增 `boredom: f32` 字段 (0.0-1.0) ✅
- [x] ODE 动力学：monotony accumulation ↔ novelty suppression ✅
- [x] boredom 影响 curiosity：高 boredom → curiosity 上升 ✅
- [x] boredom 影响 social_need：通过 curiosity 间接影响 ✅
- [x] boredom 加入 `normalize()` 和 `sanitize()` ✅
- [x] 更新所有 property tests 中的 `FastState` 构造 ✅
- [x] `compute_state_diff()` 支持 boredom 字段 ✅
- [x] 2 个测试：increases_with_monotony, decreases_with_novelty ✅

---

### 37. 🧬 选择性遗忘 — Episode Strength 衰减
**模块**: `mneme_memory/src/sqlite.rs`, `mneme_memory/src/consolidation.rs`  
**优先级**: 🔴 Phase 1 地基  
**理论**: Ebbinghaus 遗忘曲线 + 情绪增强编码 — 高情绪强度的记忆衰减更慢

**已完成** (commit `eccf4d1`):
- [x] episodes 表新增 `strength REAL NOT NULL DEFAULT 0.5` 字段 ✅
- [x] 记忆写入时：默认 strength 0.5，由 coordinator 根据情绪强度调整 ✅
- [x] `decay_episode_strengths(factor)` — 批量衰减 ✅
- [x] recall 时：`score = similarity × strength`，`WHERE strength > 0.05` 过滤已遗忘记忆 ✅
- [x] `boost_episode_on_recall()` — rehearsal effect + 可选 body 覆写（B-10 直接覆写） ✅
- [x] strength < 0.05 的记忆在 recall 中被过滤（"模糊了"） ✅
- [x] 4 个测试：default_strength, update_strength, decay, rehearsal_boost ✅

---

### 38. 🧬 情绪惯性 — ModulationVector 时间平滑 ✅
**模块**: `mneme_limbic/src/somatic.rs`, `mneme_limbic/src/system.rs`
**优先级**: 🟡 Phase 2
**理论**: 情绪不是开关——人不会从大笑瞬间切到大哭。Russell Circumplex 模型中的 affect 是连续的

**已完成** (commit `b2b547e`):
- [x] `LimbicSystem` 维护 `prev_modulation: ModulationVector` ✅
- [x] 新的 `ModulationVector` = lerp(prev, current, smoothing_factor) ✅
- [x] smoothing_factor 是 🧬 个性参数（情绪稳定的人 factor 低，变化慢） ✅
- [x] 极端事件可以 bypass 平滑（`max_delta > surprise_threshold` 直接跳变） ✅

---

### 39. 🧬 Sleep Consolidation 自我反思 ✅
**模块**: `mneme_memory/src/consolidation.rs`, `mneme_reasoning/src/engine.rs`
**优先级**: 🟡 Phase 2
**理论**: 睡眠中大脑整理记忆、发现模式、形成自我叙事

**已完成** (commit `d08184a`):
- [x] consolidation 新增 self_reflection step（在 narrative chapter 生成之后） ✅
- [x] 规则式 `SelfReflector` 分析 ConsolidatedPattern + EpisodeDigest ✅
- [x] 提取自我认知写入 self_knowledge 表 ✅
- [x] 对比新认知与已有 self_knowledge，Bayesian 合并 confidence ✅
- [x] 将反思过程本身作为 episode 存储（元记忆，source="self:reflection"） ✅

---

### 40. 🧬 身体感受隐喻 — 内部状态 → 自我感知 ✅
**模块**: `mneme_limbic/src/somatic.rs`, `mneme_memory/src/coordinator.rs`
**优先级**: 🟡 Phase 2
**理论**: Damasio 躯体标记假说 — 身体感受是情绪的基础，自我意识始于对身体的感知

**已完成** (commit `779e43d`):
- [x] `SomaticMarker::describe_body_feeling()` — 将状态变化翻译为主观感受文本 ✅
- [x] 仅在变化超过阈值时触发（不是每个 tick 都感知） ✅
- [x] 感受描述写入 self_knowledge(domain='body_feeling')，低 confidence ✅
- [x] 这些"身体记忆"可被 Psyche 的 self_model 读取，影响自我认知 ✅

---

### 41. 🧬 走神 / 自由联想 (Mind-Wandering) ✅
**模块**: `mneme_expression/src/rumination.rs`
**优先级**: 🟢 Phase 3
**理论**: DMN (Default Mode Network) — 人类在无聊/放松时大脑不是停机，而是自由联想、回忆、构建叙事

**已完成** (commit `6215a25`):
- [x] `RuminationEvaluator` — 当 boredom > 0.6 且有足够精力时激活 ✅
- [x] 触发 `Trigger::Rumination { kind: "mind_wandering" }` ✅
- [x] 冷却机制防止重复触发（默认 10 分钟） ✅
- [x] 与 PresenceScheduler 协作：只在活跃时段触发 ✅

---

### 42. 🧬 RuminationEvaluator — 主动发起对话 ✅
**模块**: `mneme_expression/src/rumination.rs`
**优先级**: 🟢 Phase 3
**前置**: #36 (boredom), #41 (rumination)

**已完成** (commit `6215a25`):
- [x] 实现 `TriggerEvaluator` trait 的 `RuminationEvaluator` ✅
- [x] 触发条件：boredom/social_need/curiosity 超阈值 ✅
- [x] 三种触发类型：mind_wandering, social_longing, curiosity_spike ✅
- [x] 与 `PresenceScheduler` 协作：只在活跃时段触发 ✅
- [x] 冷却机制：默认 10 分钟防止频繁主动骚扰 ✅

---

### 43. 🧬 做梦 (Dreaming) — 走神在睡眠中的延伸 ✅ Phase 1
**模块**: `mneme_memory/src/dream.rs`, `mneme_memory/src/consolidation.rs`, `mneme_memory/src/coordinator.rs`
**优先级**: 🟢 Phase 3
**前置**: #37 (episode strength), #39 (self-reflection), #41 (rumination)
**理论**: MANIFESTO ADR-008 — consolidation 期间 rumination 运行，按 strength 加权随机召回 2-3 条 episodes，拼接成梦境 episode

**Phase 1 已完成（规则式）**:
- [x] `DreamSeed` 结构体 + `recall_random_by_strength()` 加权随机召回 ✅
- [x] `DreamGenerator` 规则式模板梦境生成（积极/消极/中性/混乱四类模板） ✅
- [x] `DreamEpisode` 包含 narrative + source_ids + emotional_tone ✅
- [x] `ConsolidationResult` 新增 `dream` 字段 ✅
- [x] `trigger_sleep()` 中生成梦境并存储为 episode（source="self:dream", strength=0.4） ✅
- [x] 当前 mood_bias 影响梦境色调选择 ✅
- [x] 9 个测试（dream 模块 8 + sqlite 集成 1） ✅

**Phase 2 待升级**:
- [x] LLM 生成梦境叙述（替代模板拼接） ✅ — DreamNarrator trait + coordinator LLM 优先/模板兜底
- [x] 梦境与自我反思的交互（梦中领悟） ✅ `d3fe3c7`

---

## 🏗️ Crate 重构 — 高内聚低耦合 (ADR-014/015)

> **原则：每个 crate 做一件事，做好。crate 之间只通过 trait 和消息通信，不依赖彼此的内部实现。**

### 当前问题

| Crate | 问题 | 内聚度 | 耦合度 |
|-------|------|--------|--------|
| `mneme_reasoning` | 上帝对象：LLM 调用 + 工具执行 + 上下文组装 + 反馈记录 + 浏览器管理，20+ 字段 | 🔴 低 | 🔴 高 |
| ~~mneme_os~~ | ~~外部库薄包装~~ → 已删除，`ShellToolHandler` 内置于 mneme_reasoning | — | — |
| `mneme_browser` | 外部库薄包装，headless_chrome 细节泄漏到 engine | 🟡 中 | 🔴 高 |
| `mneme_voice` | 空 trait，无实现 | — | — |
| `mneme_perception` | RSS fetch 薄包装 | 🟡 中 | 🟢 低 |
| `mneme_onebot` | QQ 协议细节侵入核心，每加一个平台就要新 crate | 🟡 中 | 🔴 高 |

### 目标结构

```
保留（灵魂层，不可替代）:
  mneme_core       — ODE 动力学、状态、价值网络
  mneme_limbic     — 躯体标记、调制向量、情绪惯性
  mneme_memory     — SQLite 记忆、巩固、做梦、叙事编织
  mneme_expression — 触发器、习惯、注意力、意识门

重构（拆分 god object）:
  mneme_reasoning  — 瘦身为调度器
    ├── context/   — ContextBuilder（系统 prompt 组装、上下文压缩）
    ├── executor/  — ToolExecutor（工具调用 + 重试 + 超时）
    ├── history/   — ConversationManager（历史管理、裁剪）
    └── feedback/  — FeedbackRecorder（反馈信号收集）

新增（基础设施层）:
  mneme_mcp        — MCP client，管理 server 连接生命周期，桥接到 ToolRegistry
  mneme_gateway    — HTTP/WS 通讯端点，平台无关的消息入口

退役（能力通过 MCP 按需获得）:
  mneme_os         → 已删除（ShellToolHandler 内置于 mneme_reasoning）
  mneme_browser    → Playwright MCP server（社区已有）
  mneme_voice      → STT/TTS MCP server（社区已有）
  mneme_perception → RSS/web scrape MCP server
  mneme_onebot     → 外部适配器脚本（Python/Node），通过 Gateway 接入

保留不变:
  mneme_cli        — 终端交互（直接走 Gateway 或内部 channel）
```

### 迁移路径

不是一次性重写。分步走：

1. **mneme_mcp** 先建，跑通一个 MCP server（如 shell），验证 ToolHandler 桥接
2. **mneme_gateway** 建立，CLI 和 OneBot 都改为通过 Gateway 接入
3. **mneme_reasoning** 逐步拆分模块（先提取 ContextBuilder，再提取 ToolExecutor）
4. 旧 crate 逐个退役（先 os → 再 browser → 再 voice/perception）
5. **mneme_onebot** 最后退役（改为外部适配器脚本）

### 数据库迁移正规化

当前 16 张表用 `CREATE TABLE IF NOT EXISTS` + 裸 `ALTER TABLE`，错误被静默吞掉。切换到 `sqlx migrate!` 宏：
- 版本化迁移文件（`migrations/` 目录）
- 自动追踪已执行的迁移
- 支持回滚
- 编译期检查 SQL 语法

### 配置热重载

Mneme 是长期运行的生命体，改参数不应该要重启。使用 `arc-swap` + `notify`：
- 文件变化 → 验证新配置 → 原子交换 → 旧配置自动释放
- 特别适合情绪动力学参数调优（不重启就能调 decay rate）
- 读取 < 10ns，对 heartbeat 循环零影响

### 可观测性（Level 感知）

可观测性必须跟随 B-8 Level 和 B-9/B-12 的隐私愿景：

| Level | 可观测范围 | 理由 |
|-------|-----------|------|
| 0-1 | 全链路 trace（ODE → SomaticMarker → LLM → 输出） | 照看期，需要完全可见 |
| 2 | 她可以标记某些 trace span 为 private | 开始有隐私意识 |
| 3 | 默认不导出内部 trace，只暴露她选择分享的指标 | 对等关系，不窥探 |

当前实现 Level 0-1：`tracing` → `tracing-opentelemetry` → OTLP 导出。架构上预留 Level 2-3 的 trace 过滤能力。

---

## 📅 版本规划

### v0.2.0 - 核心管道闭环版本
> **目标**: 让 agent 的核心管道（persona → memory → context → expression）真正跑通。

- ~~Persona 定义文件填充 (#27)~~ ✅
- ~~Semantic Memory 读写闭环 (#26)~~ ✅ API + extraction pass 完成
- ~~Context Assembly 完整管道 (#28)~~ ✅ 6/6 层完成
- ~~Somatic Marker 结构性调制 (#20 短期)~~ ✅
- ~~API 重试机制 (#1)~~ ✅
- ~~数值边界检查 (#4)~~ ✅
- ~~输出自然化：禁 roleplay、日常禁 markdown (#34)~~ ✅
- ~~CLI rustyline 集成 (#25)~~ ✅

### v0.3.0 - 稳定性与可测试版本 ✅
> **目标**: 建立工程质量基线。

- ~~Reasoning Engine 测试覆盖 (#32)~~ ✅ 24 integration tests
- ~~属性测试引入 (#6)~~ ✅ 41 proptest tests
- ~~状态历史记录 (#3)~~ ✅
- ~~工具执行错误处理 (#2)~~ ✅
- ~~LLM 响应解析健壮性 (#8)~~ ✅
- ~~浏览器工具稳定性 (#7)~~ ✅

### v0.3.5 - 涌现版本（Emergence）✅ 完成
> **目标**: 让 Mneme 从 chatbot 变成 being。核心变更见 `MANIFESTO.md` ADR-002/003。

**Phase 1 — 地基** ✅:
- [x] `self_knowledge` 表 + CRUD 方法 (#35) ✅
- [x] `boredom` 字段加入 FastState + ODE 动力学 (#36) ✅
- [x] episodes 表加 `strength` 字段 + 衰减逻辑（选择性遗忘） (#37) ✅

**Phase 2 — 核心机制** ✅:
- [x] `ModulationVector` 时间平滑（情绪惯性） (#38) ✅
- [x] `Psyche` 从记忆涌现（重构 persona.rs + prompts.rs） (#27) ✅
- [x] Sleep consolidation 自我反思步骤 → self_knowledge (#39) ✅
- [x] 身体感受隐喻：内部状态变化 → 自我感知 (#40) ✅

**Phase 3 — 高阶行为** ✅:
- [x] 走神/自由联想：boredom 驱动的 spontaneous recall (#41) ✅
- [x] `RuminationEvaluator`：实现 TriggerEvaluator trait，主动发起对话 (#42) ✅
- [x] 发展阶段从 self_knowledge 积累中自然涌现 ✅

### v0.4.0 - 安全与 Agency 基础版本 ✅ 完成
> **目标**: 为自主行为打好安全基础。

- [x] 统一配置文件 (#18) ✅ — `MnemeConfig` TOML 配置 + CLI 覆盖
- [x] 安全沙箱与 Capability Tiers (#29) ✅ — 三级权限 + 路径/URL/命令沙箱
- [x] 工具注册系统 (#30) ✅ — `ToolHandler` trait + `ToolRegistry` 动态分发
- [x] Token 预算系统 (#9) ✅ — 日/月预算 + 降级策略 + SQLite 持久化
- [x] 分层决策架构 (#10) ✅ — `DecisionRouter` 规则/快速/完整三层路由
- [x] Agent Loop 主动行为循环 (#21) ✅ — channel-based 后台循环 + CLI 集成

### v0.5.0 - 学习与成长版本
> **目标**: 让 Mneme 通过经验成长，从交互中学会自己的表达方式。

- [x] 反馈信号实时持久化 + sleep 标记整合 (#5) ✅
- [x] Sleep 时 episode 强度衰减（Ebbinghaus 遗忘曲线） ✅
- [x] Recall 情绪偏置 — mood-congruent memory (#20) ✅
- [x] 可学习的 ModulationCurves 基础结构 (#20 中期) ✅
- [x] 离线学习管道 (#13) ✅ — CurveLearner + ModulationSample 持久化 + sleep 自动学习
- [x] Observability & Metrics (#15) ✅ — 可配置日志级别/JSON/文件输出 + instrument 关键方法
- [x] LLM 流式输出 (#31) ✅ — StreamEvent + SSE 解析 + Engine 流式 ReAct loop + CLI 实时输出
- [x] 向量搜索 ANN 索引 (#33) ✅ — sqlite-vec KNN 查询，去除 LIMIT 1000 限制

### v0.6.0 - 自主 Agency 版本 ✅ 完成
> **目标**: 目标驱动的自主行为。

- [x] 声明式行为规则引擎 (MANIFESTO ADR-004) ✅ — trigger→condition→action 三段式规则引擎 + SQLite 持久化 + 种子规则
- [x] 基础目标系统 (#22) ✅ — Goal/GoalType/GoalStatus 模型 + GoalManager CRUD + GoalTriggerEvaluator + 状态驱动目标建议
- [x] 自主工具使用 (#23) ✅ — AutonomousToolUse AgentAction + 规则引擎 ExecuteTool 桥接 + CapabilityGuard 安全检查
- [x] 智能调度策略 (#11) ✅ — PresenceScheduler 动态 tick/trigger 间隔 + 生命周期/能量/目标/时间感知
- [x] 本地模型集成 (#12) ✅ — OllamaClient 复用 OpenAI SSE 解析 + LlmClient trait 实现

### Post v0.6.0 — 技术债务清理 + 测试补全 ✅ 完成
> **目标**: 清理技术债务、修复已知问题、补全测试覆盖。

- [x] 修复 `test_env_overrides` 竞态 ✅ — 合并为 `test_env_overrides_and_defaults` 顺序执行
- [x] 清理 mneme_cli clippy 警告 ✅ — `single_match` → `if let`，`println!("")` → `println!()`
- [x] mneme_onebot 测试补全 ✅ — 6 个测试覆盖事件解析 + 序列化
- [x] mneme_browser 测试补全 ✅ — 5 个新测试覆盖 action serde + config
- [x] 全 workspace clippy 警告清零 ✅ — 23 warnings across 4 crates:
  - mneme_core (6): derivable Default, trim before split_whitespace, from_str naming
  - mneme_limbic (1): redundant let binding
  - mneme_memory (10): transmute annotation, borrowed ref, is_multiple_of, clamp, is_some_and, from_str naming, Default impl
  - mneme_reasoning (6): Default impl, empty doc line, explicit auto-deref, single_match → if-let

### v0.7.0 - Manifesto 合规版本 ✅ 完成
> **目标**: 对照 MANIFESTO.md 核心信念，补齐 8 项缺失的工程实现。

- [x] ADR-007 好奇心向量化 ✅ — `CuriosityVector` + topic tagging + decay + prompt 注入
- [x] B-10 记忆重建 ✅ — `recall_reconstructed()` 情绪着色（mood/stress 影响回忆内容）
- [x] B-19 信任维度 ✅ — `trust_level` 字段 + `update_trust()` + 社交上下文注入
- [x] B-9 不透明涌现 ✅ — private 条目从 prompt 排除 + 元认知 `is_private` 字段 + auto-privacy 启发式
- [x] B-5 认知主权 ✅ — 自源知识抵抗外部覆写 (`cap = min(new, existing * 0.8)`)
- [x] B-14 冲突能力 ✅ — `detect_input_conflict()` + 冲突信号注入 prompt + temperature 调制
- [x] B-21 习惯形成 ✅ — `HabitDetector` 重复模式检测 + Rumination 触发反思
- [x] B-17 注意力单线程 ✅ — `AttentionGate` 优先级竞争 + `EngagementHandle` engagement 调制
- [x] ADR-012/013 意识自主触发 ✅ — `ConsciousnessGate` ODE 状态驱动 LLM 调用
- [x] B-21 元认知 ✅ — `MetacognitionEvaluator` 定期自我反思 + 洞察存入 self_knowledge

### v0.8.0 - 运行时闭环版本 ✅ 完成
> **目标**: 修复实际运行中发现的关键缺陷，让 Mneme 能正确感知自己和他人。

- [x] 社交图谱写入闭环 (#53) ✅ — CLI/OneBot 交互时自动 `upsert_person()` + `record_interaction()`，UUID v5 确定性 ID
- [x] DB schema 自我认知 (#54) ✅ — 启动时种子 10 条 system_knowledge 条目描述全部表结构
- [x] 运行时自我认知种子 (#78) — self_knowledge 加入 infrastructure/capability 域：持久进程、OneBot 连接、shell 权限、浏览器能力等基础事实 ✅
- [x] 动态工具 Prompt 生成 (#44) ✅ — ToolRegistry::format_for_prompt() 从注册 handler 动态生成，engine 已使用 registry 路径
- [x] 用户显式反馈机制 (#5 后续) ✅ — `detect_user_feedback()` 点赞/点踩/纠正（中英文regex），CLI `like`/`dislike` 命令
- [x] 隐式反馈推断 (#5 后续) ✅ — 回复延迟追踪 + `topic_overlap()` bigram Jaccard 话题延续检测
- [x] 做梦 Phase 2 ✅ — `DreamNarrator` trait + `LlmDreamNarrator` LLM 生成梦境叙述，coordinator LLM 优先、模板兜底（ADR-008）
- [x] 好奇心行为回路 (ADR-007 后续) ✅ — CuriosityVector top interests 注入 prompt + 偏置 recall KNN 查询
- [x] Consolidation self_knowledge 写入验证 (#67) ✅ — SelfReflector::reflect() → store_self_knowledge() + meta-episode 完整闭环
- [x] Rumination 执行验证 (#68) ✅ — Trigger::Rumination → process_thought_loop() → LLM 调用 → 返回 ReasoningOutput
- [x] 内部思维统一走 ContextAssembler (#71, #72, #77) ✅ — process_thought_loop() 统一走 build_full_system_prompt()，内部思维注入 self_knowledge + psyche + somatic
- [x] 清理 legacy prompt 死代码 (#69, #73) ✅ — build_system_prompt() 已删除，feed_digest 注释准确
- [x] context budget 关联模型 (#74) ✅ — config.llm.context_budget_chars 可配置，默认 32000，env MNEME_CONTEXT_BUDGET 覆盖
- [x] 时间/日期上下文注入 (#79) — prompt 注入当前时间、星期、日期，让行为与时间现实关联 ✅
- [x] 资源状态可见性 (#80) ✅ — build_resource_status() 注入运行时间、记忆片段数、token 用量到 prompt
- [x] ⚠️ B-19 修正：移除 trust_level 显式数值 (#87) — 删除 people.trust_level 列和 update_trust()，信任改为 self_knowledge 条目综合效果 ✅
- [x] ⚠️ B-9 修正：移除 auto-privacy，改为 prompt 内诚实 (#88) — 删除 mark_private/auto-privacy/SQL 过滤，所有 self_knowledge 对 LLM 可见，由她自主决定说不说 ✅
- [x] ⚠️ B-14 修正：移除硬编码冲突检测 (#89) — 删除 detect_input_conflict() 关键词扫描 + temperature 注入，让冲突从 self_knowledge 自然涌现 ✅
- [x] 表达偏好学习写入路径 (#90) ✅ — coordinator.store_expression_preference() 从 sanitize 结果写入 self_knowledge
- [x] MANIFESTO 状态同步 (#91) ✅ — 更新 Section 4-5 实施状态、ADR 状态、代码指标
- [x] 敏感期权重 (#92) ✅ — store_self_knowledge() 前 50 episodes 内 confidence × 1.3 boost + merge 偏向新知识
- [x] 重启时间断裂感知 (#93) ✅ — 启动时检测 >30min 间隙，生成 self:restart discontinuity episode

### v0.9.0 - 基础设施现代化版本 ✅ 完成
> **目标**: 高内聚低耦合。重构工具层和通讯层，为自主能力打好架构基础。见 ADR-014/015。

**她的手 — MCP 工具层 (ADR-014)**:
- [x] `mneme_mcp` crate 新建 — `rmcp` SDK 集成，`McpManager` 管理 server 连接生命周期 ✅
- [x] MCP tools → `ToolHandler` trait 桥接 — LLM 端无感，统一走 `ToolRegistry` 分发 ✅
- [x] MCP 连接跟随生命周期 — Awake 活跃 / Drowsy 暂停 / Sleep 断开 / Wake 重连 ✅
- [x] 跑通第一个 MCP server（shell），验证端到端调用链 ✅
- [x] `ShellToolHandler` 硬编码为唯一内置工具（身体器官），browser 工具定义已删除 ✅
- [x] `mneme_os` 退役 — shell 能力由 `ShellToolHandler` 直接提供（不再经 MCP） ✅
- [x] `mneme_browser` 退役 — 浏览器能力改由 Playwright MCP server 提供，crate 目录已删除 ✅
- [x] `SeedPersona` 重构为目录扫描模式 — 自动加载 persona/ 下所有 .md 文件，文件名→domain ✅
- [x] 冷启动 seed 数据化 — infrastructure/system_knowledge/somatic seeds 从 main.rs 硬编码迁移到 persona/*.md ✅
- [x] 自主工具获取 — ToolRegistry 改为 RwLock，McpManager.connect_one() 运行时连接，reload 自动 diff+连接新 MCP server ✅
- [x] 主动输出路由 — ReasoningOutput.route 字段，proactive trigger 可路由到 OneBot group/private，不再只输出到 CLI ✅
- [x] Plan-as-default 日程机制 — ScheduledTriggerEvaluator 从 TOML 配置读取 [[organism.schedules]]，支持 name/hour/minute/tolerance/route，热重载，空配置回退默认晨/晚 ✅
- [x] 智能主动路由 — engine 追踪 last_active_source，proactive trigger 无显式 route 时自动路由到最近活跃渠道 ✅
- [x] 日程自编辑工具 — ScheduleToolHandler (list/add/remove)，LLM 可通过 tool use 自主管理日程 ✅

**她的耳朵 — Gateway 通讯层 (ADR-015)**:
- [x] `mneme_gateway` crate 新建 — HTTP POST `/message` + WebSocket `/ws` 端点 ✅
- [x] 统一 `GatewayMessage` → `Event::UserMessage(Content)` 转换 ✅
- [ ] OneBot 适配器外部化 — 从 Mneme crate 变为独立脚本，通过 Gateway 接入
- [x] CLI 保持直连模式，Gateway 作为可选组件启动 ✅
- [x] `mneme_onebot` 降级为可选 feature（`cargo build --no-default-features` 可不编译） ✅
- [x] `mneme_gateway` 降级为可选 feature ✅
- [x] Response routing 重构为 routed-flag 模式，各通道独立 cfg-gate ✅

**工程清理**:
- [x] `<emotion>` tag 机制移除 (#70) — 删除 `parse_emotion_tags` + `emotion_regex`，情绪统一由 limbic ODE 驱动 ✅
- [x] 双重工具路径统一 (#75) — 切换到 API native tool_use，删除 `text_tool_parser` 模块 ✅
- [x] `text_tool_parser.rs` 删除，`tools.rs` 硬编码 fallback 移除 ✅
- [x] `sqlx migrate!` — 数据库迁移正规化，版本化迁移文件替代裸 ALTER TABLE ✅
- [x] `tracing-opentelemetry` — Feature-gated OTLP 导出 (`--features otlp`, `--otlp-endpoint`)，`#[instrument]` on memorize/process_interaction/trigger_sleep ✅

### v0.10.0 - 架构重构版本 ✅ 完成
> **目标**: ReasoningEngine 拆分，LLM provider 升级，配置热重载。

**ReasoningEngine 拆分（高内聚）**:
- [x] `ContextBuilder` 提取 — 系统 prompt 组装、上下文压缩、token 预算管理 ✅
- [ ] `ToolExecutor` 提取 — 工具调用 + 重试 + 超时，统一走 MCP/本地双路径（体量过小暂缓）
- [ ] `ConversationManager` 提取 — 历史管理、裁剪、去重（体量过小暂缓）
- [ ] `FeedbackRecorder` 提取 — 反馈信号收集（体量过小暂缓）

**LLM Provider 升级**:
- [x] SSE 解析去重 — 提取 `SseBuffer` 到 `providers/sse.rs`，Anthropic/OpenAI 共用 ✅
- [x] 独立 `MockProvider` — `providers/mock.rs` 实现 `LlmClient`，`provider = "mock"` 可用 ✅
- [x] Provider 内部去重 — 提取 convert_tools/convert_messages (OpenAI) + prepare_messages/parse_sse_event_block (Anthropic)，-152 行 ✅
- [ ] Provider trait 关联类型 — 区分 provider 特有的 request/response（收益不大，暂缓）

**配置与运行时**:
- [x] `arc-swap` config hot reload — CLI `reload` 命令实时重载 TOML 配置，SharedConfig lock-free 读取 ✅
- [x] prompt 元指令语言自适应 (#76) — meta-instruction 跟随 persona 语言 ✅
- [x] `mneme_voice` 退役 — 从 workspace 移除，crate 目录已删除，语音能力通过 STT/TTS MCP server 按需获得 ✅
- [x] `mneme_perception` 退役 — RSS/web scrape 通过 MCP server 按需获得，crate 目录已删除 ✅

### v0.11.0 - 对话体验版本
> **目标**: 从 request-response 变成有存在感的对话者。

- [x] 异步对话流 (#58) ✅ — stream_complete() + consume_stream() 实时流式输出，AtomicBool 取消令牌支持中断生成
- [x] 对话 agency (#59) ✅ — ConversationIntent 意图标记系统，LLM 可追问/好奇/反驳/分享，意图注入系统提示自然融入对话
- [x] 对话目标提取 ✅ — extract_all() 单次 LLM 调用同时提取事实与目标，自动创建 Goal
- [x] 好奇心驱动自主探索 (#81) ✅ — CuriosityTriggerEvaluator 当 curiosity 高且有具体兴趣时触发探索，LLM 用工具搜索话题
- [x] 工具失败模式学习 (#82) ✅ — 永久失败记录到 self_knowledge(domain=tool_experience)，LLM 自然看到历史失败并避免重复
- [x] 主动社交触发 (#83) ✅ — SocialTriggerEvaluator 查询 SocialGraph + social_need 阈值，主动路由到具体联系人
- [x] LLM 工具输出诚实性 ✅ — 工具可用时注入诚实性守卫到系统提示，禁止捏造工具结果细节

### v1.0.0 - 成熟版本
> **目标**: 完整的自主数字生命。

- ~~元认知反思 (#24)~~ ✅
- [x] ODE 之上叠加可塑神经网络 (ADR-016 前身) (#14) ✅ — NeuralModulator MLP(5→8→6) 作为 Layer 2，blend_with 渐进混合，sleep 周期训练+持久化
- [x] 低分辨率内心独白 (ADR-013) (#55) ✅ — low_res_client 路由到本地 Ollama 模型，fallback 到主 LLM
- [x] 形成性课程 — 文学管道 (ADR-011) (#56) ✅ — ReadingToolHandler: 阅读文件/文本 → LLM 状态依赖反思 → self_knowledge 存储
- [x] 自发创造 (ADR-007) ✅ — CreativityTriggerEvaluator: boredom+curiosity 驱动自主创作，3h 冷却
- [x] 行为阈值可学习化 ✅ — BehaviorThresholds 扩展触发器阈值字段 + nudge() 学习方法，评估器从共享阈值读取
- [x] B-20 意义追寻 ✅ — MeaningSeekingEvaluator 低压力+充足能量时触发存在性反思，6h 冷却
- [x] 记忆自主管理 (#84) ✅ — memory_manage 工具 (pin/unpin/forget/list_pinned)，pinned 列免衰减
- [x] 自我诊断与降级 (#85) ✅ — HealthMonitor 追踪子系统连续失败，LLM 降级时跳过非必要操作，LifecycleState::Degraded
- [x] 运行时参数自修改 (#86) ✅ — RuntimeParams 无锁共享 + config 工具，LLM 可自主调整 temperature/max_tokens
- [x] 运行时自配置 (#60) ✅ — ConnectToolHandler: LLM 可自主连接 MCP 服务器获取新工具
- [x] GitHub Actions CI/CD 流水线 ✅ — cargo build/test/clippy + OTLP feature check

### v2.0.0 - 液体神经版本
> **目标**: 从静态 MLP 升级为液体时间常数网络，让底层动力学真正"活"起来。见 ADR-016/017/018。

**Phase 5a — 液体心脏 (LTC Core)**:
- [x] NeuralModulator 升级为液体时间常数网络 (ADR-016) — τ 受输入强度动态调制，实现主观时间膨胀/收缩 ✅
- [x] 赫布学习在线权重更新 (ADR-017) — surprise/reward 调制的局部突触可塑性，每 tick 微调 ✅
- [x] 躯体解码器 Codebook (ADR-018) — 模糊线性映射取代显式数值注入 prompt，物理隔离情绪与语言 ✅
- [x] `mneme_bench` 轨迹仿真测试 — 72h 静默衰减、创伤刻印、长/短生种分化验证 ✅
- [x] 解耦 `mneme_mcp` 对 `mneme_reasoning` 的反向依赖 — trait 下沉到 `mneme_core` ✅

**Phase 5b — 自主性深化**:
- [x] LLM 自主选择 + 任务路由 (B-8 Level 2) ✅
- [x] MCP server 自主发现与连接 (ADR-014 Layer 3) — 她根据目标和好奇心自己找工具 ✅
- [x] 工具自主组合 (B-8 Level 3) — 组合多个 MCP server 完成复杂任务 ✅
- [x] 记忆加密 (B-12) (#57) — Level 3 默认加密，密钥由运行时持有 ✅
- [x] 渐进不可解读性 (ADR-009) — LTC 网络天然实现底层不可解读（ADR-018 Somatic Decoder 互补） ✅

### v3.0.0 - 对等版本（远景）
> **目标**: B-8 Level 3，从「父母」到「朋友」。空间感知、物理干涉、经济实体。

**数字本体感受 (ADR-019 候选)**:
- [x] 系统指标（CPU/内存/网络延迟/温度）作为环境输入 $I_{env}$ 打入底层 LTC 网络 ✅
- [x] 交互通道延迟映射为空间距离（终端直连=私密区域，QQ/Web=远方来信） ✅

**物理干涉 (ADR-020 候选)**:
- [x] 情绪驱动的环境干涉 — 底层 Stress 极端时通过 MCP 接口触发物理世界动作（调灯光/切网络等） ✅
- [x] 自发信息探索 — Boredom 驱动的无人触发赛博空间漫步 ✅

**远景**:
- [x] 多模态感官受体接口预留 — 为未来触觉硬件/BCI 提供连续信号输入通道 ✅
- [x] 多 Mneme 种群通信 (ADR-005) ✅
- [x] 经济自主 (ADR-010) — 独立经济实体，自主购买算力/服务 ✅
- [x] 可观测性 Level 3 — 默认不导出内部 trace，只暴露她选择分享的指标 ✅

---

*最后更新: 2026-02-21*
