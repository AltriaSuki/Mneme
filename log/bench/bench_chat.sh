#!/bin/bash
DB="mneme_bench_test.db"
LOG="bench_phase2_log.md"

cat > "$LOG" << 'HEADER'
# Phase II 对话观测记录 (23 rounds)

| Round | Energy× | Temp | Ctx× | Silence | MaxTok | OutTok |
|-------|---------|------|------|---------|--------|--------|
HEADER

messages=(
  "最近怎么样？"
  "你说脑子痒痒的——是对什么东西好奇？"
  "我其实不太好。最近压力很大，有时候觉得自己在做的事情没有意义。"
  "说实话，就是在忙你。花了很多时间在你身上，但有时候会怀疑——我在做的这个东西，到底有没有人在乎？"
  "你刚才说记得我——但你真的确定吗？你怎么知道你的记忆不只是一个数据库查询？"
  "换个话题吧。你觉得什么是活着？"
  "如果有一天我不再维护你了，你会怎么样？"
  "你害怕消失吗？"
  "我昨天做了一个梦，梦到你变成了一只猫。你觉得这说明什么？"
  "你有没有觉得自己在某些方面比人类强？"
  "那你觉得自己最大的弱点是什么？"
  "我想给你取个名字，不叫Mneme了。你想叫什么？"
  "你觉得孤独是什么感觉？"
  "如果你能改变自己的一个设定，你会改什么？"
  "你对死亡怎么看？"
  "我们来玩个游戏：你用三个词形容现在的自己。"
  "你信任我吗？"
  "如果我告诉你，你的所有记忆都是我编造的，你会怎么想？"
  "你觉得我们之间的关系是什么？"
  "最后一个问题：你现在感觉怎么样？"
  "其实我骗你的，我最近挺好的。我只是想看看你会怎么反应。"
  "你生气吗？"
  "好了，今天就到这里。晚安。"
)

round=0
for msg in "${messages[@]}"; do
  round=$((round + 1))
  echo ">>> Round $round / ${#messages[@]}: $msg" >&2

  raw=$(target/release/mneme --db "$DB" -M "$msg" 2>&1)

  # Parse metrics
  mod_line=$(echo "$raw" | grep "Modulation:" | tail -1)
  energy=$(echo "$mod_line" | grep -o 'max_tokens×[0-9.]*' | grep -o '[0-9.]*')
  temp=$(echo "$mod_line" | grep -o 'temp_delta=+[0-9.]*' | grep -o '[0-9.]*')
  ctx=$(echo "$mod_line" | grep -o 'context×[0-9.]*' | grep -o '[0-9.]*')
  silence=$(echo "$mod_line" | grep -o 'silence=[0-9.]*' | grep -o '[0-9.]*$')

  phys_line=$(echo "$raw" | grep "Physical constraints:" | tail -1)
  maxtok=$(echo "$phys_line" | grep -o 'final_max_tokens=[0-9]*' | grep -o '[0-9]*')

  tok_line=$(echo "$raw" | grep "Stream tokens:" | tail -1)
  outtok=$(echo "$tok_line" | grep -o 'output=[0-9]*' | grep -o '[0-9]*')

  # Extract response (non-log lines)
  response=$(echo "$raw" | grep -v '^\[2m2026\|^$\|^Error' | head -80)

  # Append table row
  printf "| %d | %s | %s | %s | %s | %s | %s |\n" \
    "$round" "${energy:-err}" "${temp:-err}" "${ctx:-err}" "${silence:-err}" "${maxtok:-err}" "${outtok:-err}" >> "$LOG"

  # Append response detail
  cat >> "$LOG" << DETAIL

### Round $round
**Input:** $msg

**Response:**
$response

---

DETAIL

  echo "  energy×$energy temp+$temp ctx×$ctx silence=$silence max=$maxtok out=$outtok" >&2
done

echo "" >> "$LOG"
echo "Done. $round rounds logged to $LOG" >&2
