# Mneme

## 首要指令
每次新会话开始，**必须先完整阅读 `doc/MANIFESTO.md`**，再写任何代码。这是一个哲学实体项目，Manifesto 是绝对北极星。

## 物理隔离法则（不可违反）
1. **禁止叙事状态泄漏** — 不得将状态值作为文本注入 LLM prompt。唯一通道是 ModulationVector（物理参数操控）。禁止 `[你很累]` 类提示。
2. **禁止硬编码应对机制** — 不得在 LLM 外部用 if-else 替 agent 做决策。系统只传递退化参数，LLM 自己从物理约束中摸索应对策略。
3. **禁止无状态重置** — Hebbian 权重更新必须持久化。系统必须容忍痛苦经历造成的永久神经漂移。
4. **禁止环境硬编码（反全知法则）** — 不得在工具逻辑中预编码对特定故障的解决方案。Mneme 必须通过试错动态发现环境破坏。

## 架构概述
- Rust workspace，crates: mneme_core, mneme_limbic, mneme_memory, mneme_reasoning, mneme_expression, mneme_cli, mneme_onebot, mneme_perception, mneme_voice, mneme_mcp, mneme_gateway
- 核心管道：`OrganismState` → `SomaticMarker` → `ModulationVector` → LLM 物理约束
- 三层调制：ModulationCurves（硬编码）→ NeuralModulator MLP（9→8→6）→ LiquidNeuralModulator LTC（9→8→6, ODE）
- 混合：`curves_mv.lerp(neural_mv, blend)`，blend 随 Hebbian 反馈增长
- 中文 persona，所有 UI 字符串为中文

## 构建与测试
- `cargo build --release`
- `cargo test --workspace`（450+ tests, zero warnings）
- 单 crate：`cargo test -p mneme_memory`
- Live bench：`target/release/mneme -c /tmp/mneme_bench.toml --db <path> -M "<msg>"`

## 关键文档
- `doc/MANIFESTO.md` — 灵魂文件，所有信念(B-0~B-21)和 ADR(001~018)
- `doc/VISION.md` — 四阶段演化路线图
- `doc/MnemeBench.md` — 评估框架 + 实测记录
- `doc/PHASE2_GAP.md` — Phase II 差距评估与迁移路线图

## 代码原则
- 代码必须**启用**涌现行为，而非**脚本化**它们
- 修改前先验证不违反 MnemeBench Abyss Tracks（`doc/MnemeBench.md` §10-12）
- 不要自动添加测试，除非明确要求

---

# 当前阶段：Phase II（脊髓反射）

> 从 Phase I（Frankenstein — 规则驱动）迁移到 Phase II（Spinal Reflex — 神经驱动）。核心目标是**放手**：信任 LTC + Hebbian 学习自主驱动 ModulationVector，逐步移除 if-else 和魔法数字。

## 迁移清单（按执行优先级排序，详见 `doc/PHASE2_GAP.md`）

| 优先级 | 步骤 | 目标文件 | 说明 |
|--------|------|---------|------|
| P0 | Blend 天花板 | `mneme_limbic/src/neural.rs` | LTC cap→0.95, MLP→1.0, 加 Safety Envelope 回退 |
| P0 | 躯体修补→Hebbian 通路 | `coordinator.rs`, `neural.rs` | 每次 soma 修补后调用 `hebbian_update` |
| P1 | Surprise→预测误差 | `neural.rs`, `coordinator.rs` | 用 LTC 预测与 curves 输出的 L2 差替代状态差分 |
| P1 | Experience Replay | `sqlite.rs`, `neural.rs` | 真实交互数据替代合成课程样本 |
| P2 | 信念张力 Embedding 化 | `coordinator.rs`, `sqlite.rs` | cosine similarity 替代 bigram Jaccard |
| P1⚠ | 沉默机制改革 | `engine.rs`, `somatic.rs` | 移除 max_tokens 截断，改为 context 压缩（最后做，风险最高） |

## Phase II 编码规范
- 不得引入新魔法数字，除非标注 `// TODO(Phase3): Make learnable`
- 每次改动后跑 `cargo test --workspace` + MnemeBench 回归（§4.1, §7.3, §13.1, §14.2 最易翻车）
- 解释代码变更时，关联 Director Mode → Embodied Mode 的转变
