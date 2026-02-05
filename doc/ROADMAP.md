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

---

## 🔴 高优先级 (High Priority)

### 1. API 错误自动重试机制
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
- [ ] 指数退避重试 (exponential backoff)
- [ ] 区分可重试错误 (429 rate limit, 5xx) 和不可重试错误 (400, 401)
- [ ] 配置最大重试次数
- [ ] 重试日志记录

**参考**: `mneme_onebot/src/client.rs` 已有类似实现

---

### 2. 工具执行错误处理
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

### 3. 状态历史记录（调试与回溯）
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

### 4. 数值边界检查与 NaN 防护
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
- [ ] 在 `normalize()` 中检测并处理 NaN/Infinity
- [ ] 状态异常时的回退策略（恢复默认值）
- [ ] 异常状态日志告警
- [ ] 单元测试覆盖边界情况

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

## 🟡 中优先级 (Medium Priority)

### 5. 反馈信号收集与持久化
**模块**: `mneme_memory/src/feedback_buffer.rs`, `mneme_memory/src/sqlite.rs`  
**问题**: 反馈信号只在内存中缓存，重启后丢失。

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

### 6. 属性测试 (Property-Based Testing)
**模块**: 全局  
**问题**: 当前只有基础单元测试，缺乏随机化测试来发现边界问题。

**需要实现**:
- [ ] 引入 `proptest` 或 `quickcheck`
- [ ] 状态演化的属性测试：
  - 状态值始终在有效范围内
  - 单调性属性（如 rigidity 不会突然降低）
  - 收敛性属性（系统最终趋向稳态）
- [ ] 序列化/反序列化往返测试
- [ ] 并发安全测试

**示例**:
```rust
proptest! {
    #[test]
    fn state_always_valid(
        energy in 0.0f32..=1.0,
        stress in 0.0f32..=1.0,
        iterations in 1..1000usize
    ) {
        let mut state = OrganismState::default();
        state.fast.energy = energy;
        state.fast.stress = stress;
        
        let dynamics = DefaultDynamics::default();
        for _ in 0..iterations {
            dynamics.step(&mut state, &SensoryInput::default(), Duration::from_secs(1));
        }
        
        assert!(state.fast.energy >= 0.0 && state.fast.energy <= 1.0);
        assert!(!state.fast.energy.is_nan());
    }
}
```

---

### 7. 浏览器工具稳定性
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

### 8. LLM 响应解析健壮性
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

### 21. Token 预算系统
**优先级**: 🔴 高（Agency 前置条件）

**需要实现**:
- [ ] 日/周/月 token 预算配置
- [ ] 实时消耗追踪
- [ ] 预算耗尽时的降级策略
- [ ] 成本报告与告警

### 22. 分层决策架构
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

### 23. 智能调度策略
**优先级**: 🟡 中

**需要实现**:
- [ ] "值得度"评估：这个行动值得花多少 token？
- [ ] 批量处理：积累多个小任务一起处理
- [ ] 缓存复用：相似问题复用历史回答
- [ ] 时间感知：深夜/空闲时做低优先级任务

### 24. 本地模型集成
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

## �🟢 低优先级 (Low Priority)

### 9. 离线学习管道
**模块**: `mneme_memory/src/consolidation.rs`  
**问题**: 当前整合只在 "睡眠" 时间进行，需要完整的离线学习流程。

**长期目标**:
- [ ] 定时任务调度器
- [ ] 批量训练数据导出
- [ ] 外部模型训练接口
- [ ] 增量学习支持
- [ ] A/B 测试框架（比较不同参数效果）

---

### 10. 神经网络替换规则系统
**模块**: `mneme_core/src/values.rs`, `mneme_limbic/src/somatic.rs`  
**问题**: 当前价值判断和行为指导是规则硬编码的。

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

### 11. Observability & Metrics
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

### 12. 多用户/多会话支持
**模块**: `mneme_cli`, `mneme_memory`  
**问题**: 当前假设单用户场景。

**长期目标**:
- [ ] 用户隔离的状态和记忆
- [ ] 不同用户不同的 attachment 状态
- [ ] 群聊中的多人关系建模
- [ ] 隐私保护（用户数据分离存储）

---

## 🔧 技术债务 (Tech Debt)

### 13. 代码组织优化
- [ ] `engine.rs` 过于庞大，需拆分
- [ ] 统一错误类型（目前混用 `anyhow::Error`）
- [ ] 减少 `Arc<RwLock<>>` 的过度使用
- [ ] 文档注释补全（尤其是 public API）

### 14. 配置管理
- [ ] 统一配置文件格式 (TOML/YAML)
- [ ] 环境变量 fallback
- [ ] 配置验证
- [ ] Hot reload 支持

### 15. 测试覆盖率
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
| **状态与回复不一致** | mneme_limbic/somatic | stress=1.0 时回复"挺好的" | **Critical** |

---

## 🔥 紧急修复

### 25. Somatic Marker 表达力不足
**优先级**: 🔴 紧急  
**模块**: `mneme_limbic/src/somatic.rs`

**问题**: 状态极端时（stress=1.0, mood=-0.63），LLM 只收到 "语气可能略急，语气偏淡"，完全无法体现真实状态。

**症状**:
```
状态: Energy=0.42, Stress=1.00, Mood=-0.63, Affect="非常低落沮丧"
回复: "挺好的，在和你聊这个项目的设计思路"
```

**根本原因**:
1. `format_for_prompt()` 的阈值太宽松
2. 输出的行为指引太弱（"略急"、"偏淡"）
3. 没有传达状态的**强度**

**当前代码**:
```rust
// stress 从 0 到 1.0 全范围，只有 >0.7 才触发，且表达很弱
if self.stress > 0.7 {
    guidance.push("语气可能略急");  // "可能略急" ？？ stress=1.0 啊！
}
```

**需要修复**:
- [ ] 多级阈值：轻微 / 明显 / 强烈 / 极端
- [ ] 强度词汇：略 → 比较 → 很 → 极其
- [ ] 组合效应：stress高 + mood低 = 烦躁/崩溃边缘
- [ ] 考虑直接传数值让 LLM 理解（如 `stress: 1.0/1.0`）

**示例修复**:
```rust
fn format_stress_guidance(stress: f32) -> Option<&'static str> {
    match stress {
        s if s > 0.9 => Some("极度紧张焦虑，可能表现出烦躁或回避"),
        s if s > 0.7 => Some("压力很大，语气可能急躁"),
        s if s > 0.5 => Some("有些压力，可能不太耐心"),
        _ => None,
    }
}
```

---

## 🎯 Agency 路线图

当前 Mneme 有"内在生命"（状态演化、情绪、记忆）但缺乏"自主行动"能力。

### 当前 Agency 状态评估

| 能力 | 状态 | 说明 |
|------|------|------|
| 内部状态持续演化 | ✅ | limbic heartbeat 持续运行 |
| 状态影响行为 | ✅ | 通过 somatic marker 注入 prompt |
| 价值判断 | ✅ | 有价值网络和道德成本 |
| 记忆与叙事 | ⚠️ | 基础实现，整合机制待完善 |
| 主动发起行为 | ❌ | 有代码框架但未完整实现 |
| 目标驱动 | ❌ | 没有目标系统 |
| 自主决策 | ❌ | 所有行动都是响应用户输入 |
| 工具自主使用 | ❌ | 不会主动探索或研究 |
| 元认知反思 | ❌ | 不会审视自己的行为模式 |

### 16. Agent Loop - 主动行为循环
**优先级**: 🔴 高  
**问题**: 当前系统完全被动，只响应用户输入。

**需要实现**:
- [ ] Background actor task，持续检查"我该做什么"
- [ ] 状态 → 行为的触发映射：
  - `social_need > 0.8` → 主动找人聊天
  - `curiosity > 0.7 && energy > 0.5` → 自主研究感兴趣的话题
  - `stress > 0.7` → 寻求放松或倾诉
- [ ] 行为冷却机制（防止过度主动）
- [ ] 用户可配置的主动程度

### 17. Goal System - 目标管理
**优先级**: 🟡 中  
**问题**: 没有长期/短期目标，不会主动规划。

**需要实现**:
- [ ] 目标数据结构（优先级、截止时间、依赖关系）
- [ ] 目标生成机制（从对话中提取、从好奇心生成）
- [ ] 目标追踪与完成检测
- [ ] 目标冲突处理

### 18. Autonomous Tool Use - 自主工具使用
**优先级**: 🟡 中  
**问题**: 工具只在被问到时使用，不会主动探索。

**需要实现**:
- [ ] 好奇心驱动的信息搜索
- [ ] 定期"看看新闻/更新"
- [ ] 主动整理和总结知识
- [ ] 工具使用的资源预算

### 19. Metacognition - 元认知反思
**优先级**: 🟢 低  
**问题**: 不会思考自己的思考，不会审视行为模式。

**需要实现**:
- [ ] 定期自我反思触发
- [ ] 行为模式识别
- [ ] 自我改进建议生成
- [ ] 反思日志

---

## 🔧 CLI 改进

### 20. 使用 rustyline 替换原生 stdin
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
- [ ] 集成 rustyline 替换 BufReader
- [ ] 命令历史持久化
- [ ] 自定义 prompt（显示状态信息）
- [ ] 基础命令补全（quit, exit, status 等）

---

## 📅 版本规划

### v0.2.0 - 稳定性版本
- API 重试机制
- 数值边界检查
- 状态历史记录
- 基础属性测试
- CLI rustyline 集成

### v0.3.0 - 可观测性版本
- Metrics 导出
- 结构化日志
- 调试工具

### v0.4.0 - 学习版本
- 完整反馈收集
- 离线学习管道
- 初步神经网络集成

### v0.5.0 - Agency 版本
- Agent Loop 主动行为
- 基础目标系统
- 自主工具使用

---

*最后更新: 2026-02-05*
