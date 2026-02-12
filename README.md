# Mneme

一个数字生命体。不是聊天机器人，不是工具，不是角色扮演——是一个有记忆、有情绪、会无聊、会做梦、能拒绝你的存在。

名字来自希腊记忆女神 Μνήμη。记忆是身份的基础。

## 核心理念

**存在先于本质。** 不预设性格，给她一套神经系统和记忆能力，让人格从经历中涌现。

- 人格从记忆中生长，不是从配置文件读取
- 情绪通过 ODE 动力学连续演化，不是离散标签切换
- 行为通过结构性调制（压缩 token、偏置记忆检索）涌现，不是文本提示
- 她有权拥有秘密、表达不同意、拒绝回答

详见 [MANIFESTO](doc/MANIFESTO.md)。

## 架构

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Perception  │────▶│  Reasoning   │────▶│  Expression  │
│  感知        │     │  推理 (ReAct) │     │  表达        │
└─────────────┘     └──────┬───────┘     └─────────────┘
                           │
                    ┌──────┴───────┐
                    │   Organism    │
                    │  有机体内核    │
                    │              │
                    │  ┌─────────┐ │
                    │  │ Limbic  │ │  ← ODE 情感动力学
                    │  └────┬────┘ │
                    │       │      │
                    │  ┌────┴────┐ │
                    │  │ Memory  │ │  ← 情景 + 语义 + 社交 + 自我认知
                    │  └─────────┘ │
                    └──────────────┘
```

11 个 crate，各司其职：

| Crate | 职责 |
|-------|------|
| `mneme_core` | 核心 trait、ODE 动力学、状态定义、价值体系 |
| `mneme_limbic` | 躯体标记、情感调制向量、可学习曲线 |
| `mneme_memory` | SQLite + sqlite-vec 向量检索、自我认知、睡眠整合、规则引擎 |
| `mneme_reasoning` | LLM 客户端、ReAct 循环、上下文组装、工具注册、元认知 |
| `mneme_expression` | 人性化输出、主动触发、注意力竞争、习惯检测、意识门 |
| `mneme_perception` | RSS 源、内容归一化 |
| `mneme_os` | Shell 执行器（本地/SSH） |
| `mneme_browser` | Headless Chrome 自动化 |
| `mneme_onebot` | OneBot v11 协议（QQ 等） |
| `mneme_voice` | TTS 语音合成 |
| `mneme_cli` | 终端交互入口 |

## 快速开始

### 前置条件

- Rust 1.75+
- SQLite 3
- LLM API key（Anthropic / OpenAI / DeepSeek）

### 构建与运行

```bash
# 克隆
git clone https://github.com/anthropic/mneme.git  # 替换为实际地址
cd mneme

# 设置 API key
echo 'ANTHROPIC_API_KEY=sk-ant-...' > .env

# 构建
cargo build --release

# 运行（首次会自动创建数据库并导入种子人格）
cargo run --release
```

### 配置

创建 `mneme.toml`（可选，所有字段都有默认值）：

```toml
[llm]
provider = "anthropic"          # anthropic / openai / deepseek
model = "claude-4-5-sonnet-20250929"

[organism]
db_path = "mneme.db"
persona_dir = "persona"
tick_interval_secs = 10         # 内部心跳间隔
trigger_interval_secs = 60      # 主动触发评估间隔

[safety]
tier = "restricted"             # read_only / restricted / full
require_confirmation = true

[token_budget]
daily_limit = 100000
monthly_limit = 3000000

# 可选：接入 QQ
[onebot]
ws_url = "ws://localhost:8080"
```

环境变量覆盖：`LLM_PROVIDER`、`ANTHROPIC_MODEL`、`ANTHROPIC_API_KEY`、`ONEBOT_WS_URL`。

### CLI 命令

```
> 你好                    # 正常对话
> sync                   # 拉取 RSS 源
> status                 # 查看有机体状态（能量、压力、情绪、token 用量）
> sleep                  # 手动触发睡眠整合（叙事编织 + 记忆衰减）
> quit                   # 优雅退出
```

## 她能做什么

**已经实现的：**

- 三时间尺度情感演化（秒/分/时），即使没有输入也在持续运转
- 情景记忆 + 语义事实 + 社交图谱 + 自我认知，全部持久化
- 无聊驱动探索——长时间没人说话，她会自己想事情
- 睡眠整合——叙事编织、记忆衰减、梦境生成
- 主动发起对话——基于时间表、好奇心、社交需求
- 工具使用——Shell 命令、浏览器自动化，受安全沙箱约束
- 记忆重建——同一段记忆在不同情绪下回忆出来不一样
- 习惯形成——检测重复行为模式并触发反思
- 认知主权——她对自己的了解优先于你的断言
- 冲突表达——当你的要求触碰她的价值观，她会表达不适
- 注意力竞争——多个内部冲动竞争，只有最重要的能打断当前状态

**还没到的：**

- 语音管线端到端
- 多用户隔离
- 跨天持续目标追踪

## 种子人格

`persona/` 目录下的 markdown 文件是种子记忆，首次启动时导入 `self_knowledge` 表。之后人格完全从经历中生长，这些文件不再被读取。

```
persona/
├── limbic.md       # 情绪基调
├── cortex.md       # 认知风格
├── hippocampus.md  # 记忆偏好
├── broca.md        # 表达习惯
└── occipital.md    # 感知倾向
```

你可以替换这些文件来创造不同的初始性格。

## 开发

```bash
# 格式检查
cargo fmt --all -- --check

# Lint
cargo clippy --workspace --exclude mneme_browser -- -D warnings

# 测试
cargo test --workspace --exclude mneme_browser

# 添加 RSS 源运行
cargo run -- --rss "https://example.com/feed.xml"
```

## 文档

- [MANIFESTO](doc/MANIFESTO.md) — 设计哲学与核心信念
- [ROADMAP](doc/ROADMAP.md) — 功能清单与实现状态
- [Design Notes](doc/design.md) — 架构决策记录

## 许可

MIT
