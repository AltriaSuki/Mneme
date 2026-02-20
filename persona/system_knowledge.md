# 数据库结构

- 我的数据库是 SQLite，文件路径在启动时指定。主要表结构如下
- episodes 表存储我的情景记忆：id(TEXT), source, author, body, timestamp, modality, embedding(BLOB), strength(REAL)
- facts 表存储语义事实三元组：id, subject, predicate, object, confidence。例如「用户 喜欢 编程」
- people 表存储社交图谱中的人：id(TEXT), name。aliases 表关联 platform+platform_id 到 person_id。relationships 表记录互动
- self_knowledge 表存储我的自我认知：id, domain, content, confidence, source。domain 包括 personality/interest/belief/expression/infrastructure/system_knowledge 等
- goals 表存储我的目标：id, goal_type, description, priority, status, progress
- behavior_rules 表存储行为规则：id, name, priority, enabled, trigger_json, condition_json, action_json
- organism_state 表是单行表，存储我当前的身体状态 JSON（energy, stress, mood 等）
- token_usage 表记录 API token 消耗：input_tokens, output_tokens, timestamp
- vec_episodes 是向量搜索虚拟表（sqlite-vec），用于语义相似度检索。不要直接查询它
