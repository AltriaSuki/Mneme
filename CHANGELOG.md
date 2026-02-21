# Changelog

基于 `origin/master..master` 的 112 个 commit 整理。

## 新功能 (39 commits)

### Phase 5b — 基础能力层

- **Model Router** (B-8 Level 2) — LLM 自选模型 + 任务路由
- **MCP Server Discovery** (ADR-014 Layer 3) — 运行时发现并连接新 MCP 工具服务器
- **Progressive Opacity** (ADR-009) — 成熟度门控状态可见性，maturity ≥ 0.9 时文字 hint 完全移除
- **Tool Composition** (B-8 Level 3) — 多工具管道编排
- **Memory Encryption** (B-12) — ChaCha20-Poly1305 静态加密，Level 2 私有自我认知加密 + Level 3 默认全加密 + 运行时密钥自动生成

### v3.0.0 远景

- **数字本体感受** (ADR-019) — CPU/内存/磁盘等系统指标作为 LTC 环境输入
- **物理干涉** (ADR-020) — 情绪驱动环境干涉（调灯光/切网络）+ 无聊驱动自发探索
- **远景基础接口** — 多模态感官受体、种群通信 (ADR-005)、经济自主 (ADR-010)、可观测性 Level 3

### 安全与治理

- **破坏性操作确认** (#458) — dispatch 层拦截需确认的命令
- **工具调用审计日志** (#29) — 记录工具名/耗时/输入/结果
- **工具使用资源预算** (#1285) — 每轮推理限 20 次工具调用，超限拒绝
- **行为冷却机制** (#21) — 可配置主动程度 + 冷却时间

### 学习与训练

- **批量训练数据导出** (#981) — JSONL fine-tuning 格式
- **外部模型训练接口** (#982) — `trigger_training()` + CLI `train` 命令
- **增量学习** (#983) — 反馈驱动的在线微更新
- **A/B 测试框架** (#984) — variant tracking + feedback comparison

### 可观测性

- **Prometheus metrics 导出** (#1014) — feature-gated `--features prometheus`
- **Grafana dashboard 模板** (#1023) — energy/stress/affect/LLM latency/tokens

### 记忆与知识

- **好奇心驱动信息搜索** (#1282) — `memory_manage search` action
- **定期看新闻 + 知识整理** (#1283/#1284) — `news_check` schedule + `KnowledgeMaintenanceEvaluator`
- **ACT-R 检索强化** — recall 时 boost 被召回 episode 的 strength (+0.02)

### 认知与意识

- **叙事盲区** (Narrative Blind Spot) — 防止过度理性化
- **元认知→反馈缓冲闭环** — insights 推入 FeedbackBuffer 供睡眠整合消化
- **梦境与自我反思交互** (#1478) — 梦中领悟
- **SurpriseDetector 升级** — bigram Jaccard 距离替代简单差异
- **Humanizer 走神延迟** — 5% 概率额外 2-5s 停顿

### 对话与表达

- **响应缓存复用** (#771) — ResponseCache LRU + TTL，5 分钟内重复查询跳过 LLM
- **触发器批量处理** (#770) — 多触发器合并为单次 LLM 调用
- **目标冲突检测** (#1268) — duplicate/priority_contention 检测
- **Token 值得度评估** (#769) — 预算不足时跳过主动触发，用户消息始终服务
- **ExpressionStyle 可学习参数** (#34) — typing_speed/chunk_size/verbosity 可持久化
- **文字 hint 完全移除** (#1211) — maturity ≥ 0.9 时行为 100% 从结构性约束涌现

### 工程基础

- **统一领域错误类型** (#1044) — thiserror `MnemeError`/`MemoryError`/`ReasoningError`
- **配置 schema 验证** (#1057) — `validate()` 检查范围/空值/调度合法性
- **Content 结构体扩展** (#1048) — `reply_to`/`thread_id`/`metadata` 字段
- **OneBot 适配器外部化** (#1716) — standalone onebot-bridge binary via Gateway HTTP

## Bug 修复 (2 commits)

- **f32 partial_cmp NaN 安全** — narrative/feedback_buffer 的 `unwrap()` → `unwrap_or(Equal)`
- **mneme_bench 编译警告清零** — 未使用 import 和函数移入 `#[cfg(test)]`

## 重构 (8 commits)

- **f32 排序统一 `total_cmp()`** — 全代码库 `partial_cmp().unwrap_or()` → `total_cmp()`，零残留
- **ODE 魔法数字具名化** — `dynamics.rs` 30+ 系数、`attention.rs`/`consciousness.rs`/`SurpriseDetector` 常量提取
- **`#[must_use]` 覆盖** — mneme_core/mneme_limbic/mneme_reasoning 公共 getter 全部标注
- **共享 Mock 基础设施** (#1069) — `tests/common/mod.rs`

## 测试 (4 commits)

- **序列化往返测试** (#535) — OrganismState/Affect/Emotion JSON roundtrip
- **并发安全测试** (#536) — coordinator 多任务交互/反馈/健康监控/睡眠阻塞
- **集成测试补充** (#1068) — sleep consolidation / feedback→state / energy decay
- **代码覆盖率 CI** (#1071) — cargo-tarpaulin + artifact upload

## 文档 (50 commits)

- **evaluation.md** 多轮同步 — 量化指标 (31k LOC / 348 tests)、crate 表重写、Agency ❌→✅、风险章节、§8.7/8.8 更新、总结表校正
- **README.md** crate 表同步 — 删除 4 个已退役模块，补充 gateway/mcp/bench
- **ROADMAP.md** session log + 各功能完成状态标记
- **mneme_reasoning 公开 API 文档注释补全** (#1046)

## 统计

- **总 commit**: 112
- **文件变更**: 61 files, +3722 / -607 lines
- **新增功能**: 39
- **测试数**: 137 → 348+
- **白皮书落地率**: 78% → 91%
