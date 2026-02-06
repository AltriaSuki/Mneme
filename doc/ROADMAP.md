# Mneme 开发路线图 (Roadmap)

本文档记录当前已知问题和未来改进计划。

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
| `dynamics.rs` | `energy_target: 0.7` | 从反馈中学习最优目标 | 🟡 |
| `dynamics.rs` | `stress_decay_rate: 0.002` | 个体化的压力恢复速度 | 🟡 |
| `somatic.rs` | `energy < 0.3 → 简洁回复` | 学习什么状态下该简洁 | 🔴 |
| `somatic.rs` | `stress > 0.7 → 语气略急` | 学习压力如何影响表达 | 🔴 |
| `state.rs` | 行为指导文本 | 从成功交互中学习表达方式 | 🟡 |
| `values.rs` | 初始价值权重 | 从用户反馈中调整 | 🟢 |
| `prompts.rs` | 表达风格指引 | 个性化的沟通风格 | 🟡 |
| `somatic.rs` | **文字指令注入模式** | → 结构性调制（无状态 LLM 范式） | 🔴 |
| `engine.rs` | `max_tokens` 固定 | → 由 energy/stress 调制 | 🔴 |
| `engine.rs` | `temperature` 固定 | → 由 arousal/stress 调制 | 🔴 |
| `engine.rs` | 记忆召回无偏差 | → 由 mood/stress 偏置 recall | 🟡 |

### 实现路径

```
Phase 1: 参数化（当前重点）
    所有魔法数字变成配置/状态的一部分
    存储在 OrganismState.slow 或单独的 PersonalityParams 中
    
Phase 2: 可观测
    记录"规则触发 → 行为 → 用户反馈"的关联
    建立 (state, action, reward) 三元组数据集
    
Phase 3: 在线微调
    用反馈信号调整参数（简单的强化学习）
    每次正面反馈强化当前参数，负面反馈调整
    
Phase 4: 神经网络替换
    用小型网络替代规则，完全数据驱动
    从大量交互中学习 state → behavior 映射
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

`SomaticMarker` 不应被废弃，而是**职责转变**：

| | 旧角色 | 新角色 |
|---|--------|--------|
| **给谁** | 给 LLM 读的文字 | 给架构层的调制信号 |
| **产出** | `"语气可能略急"` | `ModulationVector { temp: +0.2, max_tokens: 0.6, ... }` |
| **LLM 看到** | 行为指导文本 | 极简数值（可选辅助信号） |
| **行为来源** | LLM 解读文字后"演"出来 | 结构性约束下自然涌现 |

---

## 🔴 高优先级 (High Priority)

### 1. 🏗️ API 错误自动重试机制
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

### 2. 🏗️ 工具执行错误处理
**模块**: `mneme_reasoning/src/engine.rs`  
**问题**: 工具执行失败时，错误信息可能不够清晰，且没有重试机制。

**当前问题**:
- 浏览器操作失败时只返回简单错误字符串
- shell 命令超时后无法恢复
- 没有工具执行的统一错误类型

**需要实现**:
- [ ] 工具执行重试机制（对于临时性失败）
- [ ] 结构化错误返回 (`ToolResult.is_error`)
- [ ] 工具执行超时可配置化
- [ ] 浏览器会话恢复机制

---

### 3. 🏗️ 状态历史记录（调试与回溯）
**模块**: `mneme_memory/src/coordinator.rs`, `mneme_memory/src/sqlite.rs`  
**问题**: 当前只保存最新状态快照，无法回溯历史状态变化。

**需要实现**:
- [ ] 状态快照历史表 (`organism_state_history`)
- [ ] 定期记录 (每 N 分钟或每次显著变化)
- [ ] 状态 diff 计算与存储
- [ ] 调试时可查询特定时间点的状态
- [ ] 自动清理过旧历史（保留策略）

**Schema 草案**:
```sql
CREATE TABLE organism_state_history (
    id INTEGER PRIMARY KEY,
    timestamp INTEGER NOT NULL,
    state_json TEXT NOT NULL,
    trigger TEXT,  -- 'tick', 'interaction', 'consolidation'
    diff_summary TEXT
);
```

---

### 4. 🏗️ 数值边界检查与 NaN 防护
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
- [ ] 统一的 `SafeF32` 类型或 validate 宏
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

### 26. 🏗️ Semantic Memory 读写闭环
**模块**: `mneme_memory/src/sqlite.rs`, `mneme_memory/src/coordinator.rs`  
**问题**: `semantic_facts` 表已存在于 SQLite schema 中，但**没有任何代码实际读写事实三元组**。当前 `recall()` 只做 episode 向量搜索，不查询 facts、不查询 social graph、不融合 feed digest。agent 只能"检索到说过什么"，不能"知道什么"——记忆系统最核心的价值尚未兑现。

**设计文档 §4.1-4.2 要求 vs 当前状态**:

| 记忆系统 | 状态 | 说明 |
|----------|------|------|
| Episodic Memory | ✅ | 向量搜索可用 |
| Semantic Memory (fact triples) | ✅ | store/recall/decay/format 已实现 |
| Social Memory (人物图谱) | ⚠️ | 有 trait + 表，未连通 |
| Blended Recall | ❌ | 只返回 episodes |

**需要实现**:
- [x] `store_fact()` / `update_fact()` — 写入事实三元组 (subject, predicate, object, confidence) ✅
- [x] `recall_facts()` — 根据主题/关键词召回相关事实 ✅
- [x] 事实冲突检测与更新（重复三元组自动合并 confidence） ✅
- [x] `get_facts_about()` — 按主题查询事实 ✅
- [x] `decay_fact()` — 事实衰减（矛盾信息出现时降低 confidence） ✅
- [x] `format_facts_for_prompt()` — 格式化事实供 prompt 注入 ✅
- [x] 对话后的 fact extraction pass（`extraction.rs` + `extract_facts()` + think() 集成） ✅
- [ ] Social graph 的实际读写（当前只有 trait 骨架）
- [ ] `Coordinator::recall()` 返回混合结果：episodes + facts + social context

---

### 27. 🏗️🧬 Persona 定义文件
**模块**: `persona/*.md`, `mneme_core/src/persona.rs`  
**问题**: 5 个 persona 文件（hippocampus、limbic、cortex、broca、occipital）**全部为空**。`PersonaLoader` 优雅返回空字符串，但这意味着 agent 在无身份状态下运行。

**影响**: Persona 是 context assembly 的最高优先级项（设计文档 §5.2："Always present, always first"）。缺少 persona = 没有性格、没有行为边界、没有一致的表达风格。这是当前 agent 表现远低于架构能力上限的**首要原因**。

**说明**: persona 加载机制是 🏗️ 基础设施；persona 文件的内容是 🧬 个性参数——不同内容定义不同的"人格"，这正是"每个 Mneme 独特"的起点。

**需要实现**:
- [x] 设计并填充 5 个脑区 persona 文件内容 ✅（已设定为"刚出生的小女孩"人格）
  - `cortex.md` — 认知风格、推理偏好、知识领域
  - `broca.md` — 语言风格、遣词习惯、语气基调
  - `limbic.md` — 情绪倾向、情感表达方式、依恋风格
  - `hippocampus.md` — 记忆偏好、叙事风格、时间感知
  - `occipital.md` — 感知偏好、注意力分配、审美倾向
- [ ] Persona 内容的版本管理（不同"性格模板"可选）
- [ ] Persona 内容影响状态初始值（不同 persona → 不同初始 energy_target 等）

---

### 28. 🏗️ Context Assembly 完整管道
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

### 29. 🏗️ 安全沙箱与 Capability Tiers
**模块**: `mneme_os/src/local.rs`, `mneme_reasoning/src/engine.rs`  
**问题**: 设计文档 §7.1 定义了四级权限体系，§8 定义了安全措施，**目前全部未实现**。shell 可执行任意命令，无路径沙箱，无网络白名单。

**设计文档 §7.1 Capability 体系 vs 当前状态**:

| 级别 | 行为 | 当前状态 |
|------|------|----------|
| Passive（无需确认） | 读文件、抓网页、查记忆 | ❌ 未区分 |
| Active（隐式确认） | 创建文件、代发消息 | ❌ 未区分 |
| Destructive（显式确认） | 删除文件、改系统配置 | ❌ 未区分 |
| Blocked（永不允许） | 任意 shell、敏感路径 | ❌ **当前可执行任意命令** |

**安全风险**:
- `LocalShell::execute()` 可运行任何命令，包括破坏性操作
- 无路径白名单/黑名单
- 无出站网络域名白名单
- LLM 可通过工具调用执行未经审查的操作

**需要实现**:
- [ ] `CapabilityPolicy` 配置：每个工具声明所需权限级别
- [ ] 运行时权限检查（工具执行前拦截）
- [ ] 路径沙箱：文件操作限制在声明的目录内
- [ ] 命令白名单/黑名单（或正则过滤）
- [ ] 网络出站域名白名单
- [ ] Destructive 操作的用户确认流程
- [ ] 审计日志：所有工具调用记录

---

## 🟡 中优先级 (Medium Priority)

### 5. 🧬 反馈信号收集与持久化
**模块**: `mneme_memory/src/feedback_buffer.rs`, `mneme_memory/src/sqlite.rs`  
**问题**: 反馈信号只在内存中缓存，重启后丢失。这是实现个性化学习的核心数据源。

**当前状态**:
- `FeedbackBuffer` 在内存中
- 重启前调用 `shutdown()` 会尝试保存，但可能遗漏
- 没有主动收集用户反馈的机制

**需要实现**:
- [ ] 反馈信号实时持久化到 SQLite
- [ ] 启动时加载未整合的信号
- [ ] 用户显式反馈机制（点赞/点踩/纠正）
- [ ] 隐式反馈推断（用户是否继续话题、回复速度等）

**相关表**: `feedback_signals` 已存在但未完全使用

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
- [ ] 序列化/反序列化往返测试
- [ ] 并发安全测试

---

### 7. 🏗️ 浏览器工具稳定性
**模块**: `mneme_browser/src/client.rs`  
**问题**: headless_chrome 可能不稳定，没有健康检查和会话恢复。

**当前问题**:
- 浏览器崩溃后无法自动恢复
- 长时间无操作后连接可能失效
- 没有超时处理

**需要实现**:
- [ ] 浏览器健康检查 (heartbeat)
- [ ] 自动重启崩溃的浏览器
- [ ] 操作级别的超时配置
- [ ] 考虑切换到 `chromiumoxide`（更活跃维护）
- [ ] 可选的非 headless 模式用于调试

---

### 8. 🏗️ LLM 响应解析健壮性
**模块**: `mneme_reasoning/src/engine.rs`  
**问题**: LLM 输出格式可能不符合预期，解析可能失败。

**当前风险**:
- `<emotion>` 标签解析依赖正则
- 工具调用的 JSON 可能格式错误
- `[SILENCE]` 检测过于简单

**需要实现**:
- [ ] JSON 工具输入的 schema 验证
- [ ] 格式错误时的回退策略
- [ ] 多次解析失败后请求 LLM 重新格式化
- [ ] 响应内容的安全检查（防注入）

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
- [ ] 上下文感知：根据 `Content.source`（cli/qq/群聊）决定是否启用 sanitize
- [ ] 技术讨论时自动跳过 sanitize（保留代码块等）
- [ ] 🧬 不同实例的表达风格差异（有的简洁有的啰嗦，作为可学习参数）

---
### 30. 🏗️ 工具注册系统
**模块**: `mneme_reasoning/src/tools.rs`, `mneme_reasoning/src/engine.rs`  
**问题**: 工具列表硬编码在 `available_tools()` 中，工具分发是 `match` 语句。添加新工具需要修改核心引擎代码，违背设计文档 §5.4 和 §7.2 的注册式设计。

**当前状态**:
- `tools.rs` — 硬编码 `vec![shell_tool(), browser_goto_tool(), ...]`
- `engine.rs` — `match tool_name { "shell" => ..., "browser_goto" => ... }`
- 每加一个工具都要改两处核心代码

**需要实现**:
- [ ] `ToolRegistry` 结构体：运行时注册/注销工具
- [ ] `Tool` trait：`name()`, `schema()`, `capability_level()`, `execute()`
- [ ] 启动时从配置自动发现和加载工具
- [ ] 工具的 capability 级别声明（与 #29 联动）
- [ ] 插件式扩展：外部 crate 可注册自定义工具

---

### 31. 🏗️ LLM 流式输出
**模块**: `mneme_reasoning/src/providers/`, `mneme_reasoning/src/engine.rs`  
**问题**: LLM 响应完全缓冲后才处理。用户需要等待整个生成完成。对于长回复（尤其通过 OneBot 发送），严重损害"类人感"。

**当前行为**: 请求 → 等待完整响应 → 一次性处理 → 发送  
**目标行为**: 请求 → 流式接收 → 逐段处理 → 分批发送（配合 humanizer 的消息拆分）

**需要实现**:
- [ ] `LlmClient` trait 增加 `stream_completion()` 方法
- [ ] Anthropic/OpenAI provider 实现 SSE 流式解析
- [ ] Engine 层逐 chunk 处理，支持"边生成边拆分消息"
- [ ] 工具调用的流式检测（部分 JSON 缓冲直到完整）
- [ ] 流式输出与 humanizer 的 typing delay 自然配合

---

### 32. 🏗️ Reasoning Engine 测试覆盖 ✅
**模块**: `mneme_reasoning/`  
**问题**: ~~系统中最复杂的模块（ReAct 循环、工具分发、历史管理、反馈记录）**零测试**。这是整个项目最大的工程风险。~~

**当前测试分布**:

| Crate | 测试数 | 覆盖质量 |
|-------|--------|----------|
| mneme_core | 16 | ✅ 完善 |
| **mneme_reasoning** | **34** | ✅ **10 unit + 24 integration** |
| mneme_memory | 19 | ✅ 完善 |
| mneme_limbic | 9 | ✅ 良好 |
| mneme_expression | 14 | ✅ 良好 |
| mneme_perception | 4 | ✅ 良好 |
| mneme_os | 4 | ✅ 良好 |
| **mneme_onebot** | **0** | ❌ 零测试（待后续补充） |
| **mneme_browser** | **0** | ❌ 零测试 |
| mneme_voice | 0 | — 只有 trait |
| mneme_cli | 0 | ❌ 无集成测试 |

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
- [ ] OneBot 事件解析与发送测试
- [ ] GitHub Actions CI/CD 流水线配置

---

### 33. 🏗️ 向量搜索 ANN 索引
**模块**: `mneme_memory/src/sqlite.rs`  
**问题**: 当前实现线性扫描最近 1,000 条 episode 计算余弦相似度（O(n)）。数据量增长后性能线性退化，无法支撑长期记忆。

**需要实现**:
- [ ] 评估 SQLite 向量扩展（sqlite-vss / sqlite-vec）
- [ ] 或迁移到支持 ANN 的后端（LanceDB / Qdrant / pgvector）
- [ ] HNSW 索引用于近似最近邻搜索
- [ ] 索引增量更新（新 episode 插入时自动维护索引）
- [ ] 性能基准测试：1k / 10k / 100k episodes 的召回延迟

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

### 9. 🏗️ Token 预算系统
**优先级**: 🔴 高（Agency 前置条件）

**需要实现**:
- [ ] 日/周/月 token 预算配置
- [ ] 实时消耗追踪
- [ ] 预算耗尽时的降级策略
- [ ] 成本报告与告警

### 10. 🏗️ 分层决策架构
**优先级**: 🔴 高

**核心思想**: 用便宜的方式做初步判断，只在必要时调用大模型。

```
Layer 0: 规则系统（免费）
    ↓ 需要理解时
Layer 1: 本地小模型 / Embedding（极低成本）
    ↓ 需要复杂推理时
Layer 2: 云端大模型（高成本）
```

**需要实现**:
- [ ] 本地 embedding 模型做语义判断（已有 fastembed）
- [ ] 本地小型 LLM（如 Phi-3, Qwen2-0.5B）做简单决策
- [ ] 规则系统预筛选（是否值得花 token）
- [ ] 决策路由器

### 11. 🧬 智能调度策略
**优先级**: 🟡 中  
**说明**: 调度决策（何时行动、什么优先）应成为可学习的个性参数。

**需要实现**:
- [ ] "值得度"评估：这个行动值得花多少 token？
- [ ] 批量处理：积累多个小任务一起处理
- [ ] 缓存复用：相似问题复用历史回答
- [ ] 时间感知：深夜/空闲时做低优先级任务

### 12. 🏗️ 本地模型集成
**优先级**: 🟡 中

**选项**:
1. **llama.cpp / Ollama** - 本地运行开源模型
2. **candle** - Rust 原生推理
3. **ONNX Runtime** - 跨平台模型部署

**适用场景**:
- 情感分析（是否需要回应）
- 简单问答
- 内容分类
- 意图识别

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

## 🟢 低优先级 (Low Priority)

### 13. 🧬 离线学习管道
**模块**: `mneme_memory/src/consolidation.rs`  
**问题**: 当前整合只在 "睡眠" 时间进行，需要完整的离线学习流程。这是实现个性化的核心机制。

**长期目标**:
- [ ] 定时任务调度器
- [ ] 批量训练数据导出
- [ ] 外部模型训练接口
- [ ] 增量学习支持
- [ ] A/B 测试框架（比较不同参数效果）

---

### 14. 🧬 神经网络替换规则系统
**模块**: `mneme_core/src/values.rs`, `mneme_limbic/src/somatic.rs`  
**问题**: 当前价值判断和行为指导是规则硬编码的。最终目标：完全数据驱动的个性化行为。

**长期目标**:
- [ ] 研究适合的小型神经网络架构
- [ ] 在线微调机制
- [ ] 可解释性保证
- [ ] 与规则系统的平滑切换

**参考技术**:
- Burn/Candle 作为 Rust ML 框架
- ONNX 模型加载
- 强化学习从人类反馈 (RLHF)

---

### 15. 🏗️ Observability & Metrics
**模块**: 全局  
**问题**: 缺乏运行时监控和性能指标。

**需要实现**:
- [ ] Prometheus metrics 导出
  - API 调用延迟/成功率
  - 状态值分布
  - 内存使用
- [ ] Structured logging (JSON)
- [ ] Distributed tracing (OpenTelemetry)
- [ ] Grafana dashboard 模板

---

### 16. 🧬 多用户/多会话支持
**模块**: `mneme_cli`, `mneme_memory`  
**问题**: 当前假设单用户场景。每个用户关系应该是独特的个性化体验。

**长期目标**:
- [ ] 用户隔离的状态和记忆
- [ ] 不同用户不同的 attachment 状态
- [ ] 群聊中的多人关系建模
- [ ] 隐私保护（用户数据分离存储）

---

## 🔧 技术债务 (Tech Debt)

### 17. 🏗️ 代码组织优化
- [ ] `engine.rs` 过于庞大（~430 LOC），需拆分为 `ContextAssembler`、`ToolDispatcher`、`ConversationManager`
- [ ] 统一错误类型（目前混用 `anyhow::Error`；`thiserror` 已引入但未使用，无自定义错误类型）
- [ ] 减少 `Arc<RwLock<>>` 的过度使用（coordinator 有 8 个 Arc 字段，考虑 actor/mpsc 模式）
- [ ] 文档注释补全（尤其是 public API；reasoning 和 CLI 模块注释稀疏）
- [ ] `values.rs` 和 `somatic.rs` 的中文情感关键词分析逻辑重复，应抽取为共用模块
- [ ] `ContentItem` 缺少设计文档 §3.2 要求的 reply chain、thread ID、modality metadata 字段
- [ ] `headless_chrome` 同步 API 在异步上下文中直接调用，阻塞 tokio 运行时，需用 `spawn_blocking` 包装
- [ ] 浏览器操作使用了已废弃的 `wait_for_initial_tab()` API

### 18. 🏗️ 配置管理
- [ ] 统一配置文件格式 (TOML/YAML)
- [ ] 环境变量 fallback
- [ ] 配置验证
- [ ] Hot reload 支持

### 19. 🏗️ 测试覆盖率
- [ ] 集成测试补充
- [ ] Mock 基础设施完善
- [ ] CI/CD 配置
- [ ] 代码覆盖率报告

---

## 📋 已知 Bug

| Bug | 模块 | 描述 | 状态 |
|-----|------|------|------|
| Browser session lost | mneme_browser | 长时间不用后会话丢失 | Open |
| Shell timeout recovery | mneme_os | 命令超时后无法恢复 | Open |
| Memory leak in history | mneme_reasoning | history 虽有 prune 但仍可能积累 | Investigating |
| CLI 光标无法左右移动 | mneme_cli | 使用 tokio BufReader，不支持行编辑 | Open |
| CLI 中文删除残留 | mneme_cli | 删除中文字符时显示残留（实际已删除） | Open |
| ~~状态与回复不一致~~ | mneme_limbic/somatic | stress=1.0 时回复"挺好的" → 已改为结构性调制 | **Fixed** ✅ |
| ~~Persona 文件全部为空~~ | persona/*.md | 5 个脑区定义文件已填充（刚出生的小女孩人格） | **Fixed** ✅ |
| Semantic facts 读写已实现 | mneme_memory | store/recall/decay/format 方法完成 | **Fixed** ✅ |
| ~~Context assembly 管道断裂~~ | mneme_reasoning | 6/6 层上下文全部完成 | **Fixed** ✅ |
| 浏览器阻塞异步运行时 | mneme_browser | headless_chrome 同步调用阻塞 tokio | Open |
| Shell 无权限控制 | mneme_os | LocalShell 可执行任意命令，无沙箱 | Open |
| ~~输出含 roleplay 动作描写~~ | prompts/broca | `*感觉有点熟悉*` 等星号旁白，人类不这样聊天 | **Fixed** ✅ |
| ~~日常聊天用 markdown~~ | prompts/broca | 聊天中使用加粗/列表/标题，不自然 | **Fixed** ✅ |

---

## 🔥 紧急修复

### 20. 🧬 Somatic Marker 表达与参数化 → 结构性调制
**优先级**: 🔴 紧急  
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
- [ ] `ContextAssembler` 根据 `context_budget_factor` 裁剪上下文量

**中期：可学习的调制曲线（🧬 个性参数）**:
- [ ] `ModulationCurves` 结构体：定义 state → parameter 的映射函数
  ```rust
  struct ModulationCurves {
      // 每条曲线是 (input_state, output_delta) 的可学习映射
      energy_to_max_tokens: CurveParams,    // 低精力 → 少 tokens
      stress_to_temperature: CurveParams,    // 高压力 → 高 temperature
      mood_to_recall_bias: CurveParams,      // 低情绪 → 偏向负面记忆
      arousal_to_typing_speed: CurveParams,  // 高唤醒 → 快发送
      social_need_to_silence: CurveParams,   // 低社交 → 易沉默
  }
  ```
- [ ] 存储到 `OrganismState.slow` 或独立的 `PersonalityParams`
- [ ] 不同实例的曲线不同（敏感型 vs 坚韧型 vs 戏剧化型）
- [ ] 从反馈中调整曲线参数

**长期：完全数据驱动**:
- [ ] 神经网络直接从 `OrganismState` 输出 `ModulationVector`
- [ ] 用 (state, modulation, user_feedback) 三元组在线学习
- [ ] 文字 hint 完全移除，行为 100% 从结构性约束涌现

**当前行动**:
- [x] 紧急：实现 `ModulationVector` + `to_modulation_vector()` ✅
- [x] 紧急：`LlmClient` trait 的 `complete()` 支持可变 `temperature` 和 `max_tokens` ✅
- [ ] 同步：`ContextAssembler` 支持 `context_budget_factor` 裁剪
- [ ] 后续：设计 `ModulationCurves`，整合到学习循环

---

## 🎯 Agency 路线图

当前 Mneme 有"内在生命"（状态演化、情绪、记忆）但缺乏"自主行动"能力。

### 当前 Agency 状态评估

| 能力 | 状态 | 说明 |
|------|------|------|
| 内部状态持续演化 | ✅ | limbic heartbeat 持续运行 |
| 状态影响行为 | ⚠️ | 通过文字 hint 注入 prompt（导演模式），需迁移到结构性调制 |
| 价值判断 | ✅ | 有价值网络和道德成本 |
| 记忆与叙事 | ⚠️ | 基础实现，整合机制待完善 |
| 主动发起行为 | ❌ | 有代码框架但未完整实现 |
| 目标驱动 | ❌ | 没有目标系统 |
| 自主决策 | ❌ | 所有行动都是响应用户输入 |
| 工具自主使用 | ❌ | 不会主动探索或研究 |
| 元认知反思 | ❌ | 不会审视自己的行为模式 |

### 21. 🧬 Agent Loop - 主动行为循环
**优先级**: 🔴 高  
**问题**: 当前系统完全被动，只响应用户输入。  
**说明**: 主动行为的触发条件和频率应该是可学习的个性参数。

**需要实现**:
- [ ] Background actor task，持续检查"我该做什么"
- [ ] 状态 → 行为的触发映射（**这些阈值应可学习**）：
  - `social_need > threshold_social` → 主动找人聊天
  - `curiosity > threshold_curiosity && energy > threshold_energy` → 自主研究
  - `stress > threshold_stress` → 寻求放松或倾诉
- [ ] 行为冷却机制（防止过度主动）
- [ ] 用户可配置的主动程度

### 22. 🧬 Goal System - 目标管理
**优先级**: 🟡 中  
**问题**: 没有长期/短期目标，不会主动规划。  
**说明**: 目标选择和优先级应反映个体价值观和经历。

**需要实现**:
- [ ] 目标数据结构（优先级、截止时间、依赖关系）
- [ ] 目标生成机制（从对话中提取、从好奇心生成）
- [ ] 目标追踪与完成检测
- [ ] 目标冲突处理

### 23. 🧬 Autonomous Tool Use - 自主工具使用
**优先级**: 🟡 中  
**问题**: 工具只在被问到时使用，不会主动探索。  
**说明**: 探索倾向和工具偏好应该因实例而异。

**需要实现**:
- [ ] 好奇心驱动的信息搜索
- [ ] 定期"看看新闻/更新"
- [ ] 主动整理和总结知识
- [ ] 工具使用的资源预算

### 24. 🧬 Metacognition - 元认知反思
**优先级**: 🟢 低  
**问题**: 不会思考自己的思考，不会审视行为模式。  
**说明**: 反思频率和深度是个性特征。

**需要实现**:
- [ ] 定期自我反思触发
- [ ] 行为模式识别
- [ ] 自我改进建议生成
- [ ] 反思日志

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
- [ ] 自定义 prompt（显示状态信息）
- [ ] 基础命令补全（quit, exit, status 等）

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

### v0.3.0 - 稳定性与可测试版本
> **目标**: 建立工程质量基线。

- ~~Reasoning Engine 测试覆盖 (#32)~~ ✅ 24 integration tests (Mock LLM/Memory/Executor, 8 categories)
- ~~属性测试引入 (#6)~~ ✅ 41 proptest tests (ODE stability, ModulationVector bounds, sanitize idempotency)
- 状态历史记录 (#3)
- 工具执行错误处理 (#2)
- LLM 响应解析健壮性 (#8)
- 浏览器工具稳定性 (#7)

### v0.4.0 - 安全与架构版本
> **目标**: 为 Agency 打好安全基础。

- 安全沙箱与 Capability Tiers (#29)
- 工具注册系统 (#30)
- Token 预算系统 (#9)
- 分层决策架构 (#10)
- 统一配置文件 (#18)

### v0.5.0 - 学习与个性化版本
> **目标**: 让每个 Mneme 通过学习变得独特。

- 反馈信号收集与持久化 (#5)
- 离线学习管道 (#13)
- LLM 流式输出 (#31)
- Observability & Metrics (#15)
- 向量搜索 ANN 索引 (#33)

### v0.6.0 - Agency 版本
> **目标**: 从被动响应到主动行为。

- Agent Loop 主动行为循环 (#21)
- 基础目标系统 (#22)
- 自主工具使用 (#23)
- 智能调度策略 (#11)
- 本地模型集成 (#12)

### v1.0.0 - 成熟版本
- 元认知反思 (#24)
- 神经网络替换规则系统 (#14)
- 多用户/多会话支持 (#16)

---

*最后更新: 2026-02-07*
