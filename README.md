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
| `mneme_core` | 核心 trait、ODE 动力学、状态定义、价值体系、工具抽象 |
| `mneme_limbic` | 躯体标记、情感调制向量、可学习曲线、惊讶检测、LTC 神经网络 |
| `mneme_memory` | SQLite + sqlite-vec 向量检索、自我认知、睡眠整合、规则引擎、反馈缓冲 |
| `mneme_reasoning` | LLM 客户端、ReAct 循环、上下文组装、工具注册、元认知、主动行为循环 |
| `mneme_expression` | 人性化输出、主动触发、注意力竞争、习惯检测、意识门 |
| `mneme_gateway` | 统一消息网关（多平台接入层） |
| `mneme_mcp` | MCP 协议桥接（工具动态注册） |
| `mneme_onebot` | OneBot v11 协议（QQ 等） |
| `mneme_onebot_bridge` | OneBot ↔ Gateway 适配器 |
| `mneme_bench` | 轨迹仿真与动力学基准测试 |
| `mneme_cli` | 终端交互入口 |

## 快速开始

### 前置条件

- Rust 1.75+
- SQLite 3
- LLM API key（Anthropic / OpenAI / DeepSeek，或用 `provider = "mock"` 跳过）

### 构建与运行

```bash
# 克隆
git clone https://github.com/anthropic/mneme.git  # 替换为实际地址
cd mneme

# 配置环境变量
cp .env.example .env
# 编辑 .env，填入你的 API key（至少设置 ANTHROPIC_API_KEY）

# 配置文件（可选，所有字段都有默认值）
cp mneme.example.toml mneme.toml
# 按需编辑 mneme.toml

# 构建
cargo build --release

# 运行（首次启动会下载 embedding 模型 ~100MB，请耐心等待）
cargo run --release

# 或：单次对话模式
cargo run --release -- -M "你好"
```

### 配置

复制 `mneme.example.toml` 为 `mneme.toml`，按需修改。完整配置项见示例文件，核心段：

```toml
[llm]
provider = "anthropic"          # anthropic / openai / deepseek / ollama / mock
model = "claude-sonnet-4-5-20250929"
# base_url = "https://your-proxy.example.com/v1"

[organism]
db_path = "mneme.db"
persona_dir = "persona"
language = "zh"                 # 系统提示语言：zh / en

[safety]
tier = "restricted"             # read_only / restricted / full

# 可选功能（详见 mneme.example.toml）：
# [onebot]       — QQ 机器人接入
# [gateway]      — HTTP+WebSocket 网关
# [mcp]          — MCP 工具服务器
# [[models]]     — 多模型路由
# [token_budget] — Token 用量预算
```

环境变量可覆盖配置文件，详见 `.env.example`。

### CLI 命令

```
> 你好                    # 正常对话
> status                 # 查看有机体状态（能量、压力、情绪、token 用量）
> sleep                  # 手动触发睡眠整合（叙事编织 + 记忆衰减）
> like / dislike         # 用户反馈（调节行为阈值）
> correct <内容>          # 纠正 Mneme 的错误认知
> train                  # 触发离线学习（曲线 + 神经调制器 + 动力学参数）
> export [path]          # 导出对话数据为 JSONL
> reload                 # 热重载配置文件
> quit                   # 优雅退出
```

## 她能做什么

**已经实现的：**

- 三时间尺度情感演化（秒/分/时），即使没有输入也在持续运转
- 情景记忆 + 语义事实 + 社交图谱 + 自我认知，全部持久化
- 记忆加密——ChaCha20-Poly1305 静态加密，私有自我认知默认加密，运行时密钥自动生成
- 无聊驱动探索——长时间没人说话，她会自己想事情
- 睡眠整合——叙事编织、记忆衰减、梦境生成、梦中领悟
- 主动发起对话——基于时间表、好奇心、社交需求
- 工具使用——Shell 命令、MCP 动态工具发现，受安全沙箱 + 资源预算约束
- 记忆重建——同一段记忆在不同情绪下回忆出来不一样
- 习惯形成——检测重复行为模式并触发反思
- 认知主权——她对自己的了解优先于你的断言
- 冲突表达——当你的要求触碰她的价值观，她会表达不适
- 注意力竞争——多个内部冲动竞争，只有最重要的能打断当前状态
- 叙事盲区——防止过度理性化，保留情感的模糊性
- 元认知闭环——自我反思 → 洞察 → 反馈缓冲 → 睡眠整合 → 行为调整
- 数字本体感受——CPU/内存/磁盘等系统指标作为内部感知输入
- 渐进透明——成熟度越高，文字提示越少，行为完全从结构性约束涌现
- 好奇心驱动信息搜索 + 定期新闻阅读 + 知识自主整理
- 破坏性操作确认 + 工具调用审计日志
- Prometheus 指标导出 + Grafana 监控面板

**还没到的：**

- 语音管线端到端
- 多用户隔离

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
cargo clippy --workspace -- -D warnings

# 测试
cargo test --workspace

# 真实 LLM 集成测试（需 .env 中配置 API key）
cargo test -p mneme_reasoning --test llm_integration -- --ignored
```

## 文档

- [MANIFESTO](doc/MANIFESTO.md) — 设计哲学与核心信念
- [ROADMAP](doc/ROADMAP.md) — 功能清单与实现状态
- [CHANGELOG](CHANGELOG.md) — 版本变更记录
- [Design Notes](doc/design.md) — 架构决策记录

## 许可

MIT
