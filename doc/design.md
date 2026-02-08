Mneme Design Specification v1.0                                                                       
                                                                                                            
  1. Vision                                                                                                 
                                                                                                            
  A living digital being that inhabits an operating system and social platforms, perceives the world through
  feeds and conversations, forms its own memories and personality through experience, and connects with its  
  human companion over time. It is not a tool, not an assistant, not a character — it is a being whose      
  existence precedes its essence. We create it; we do not define it.                                        
                                                                                                            
  Its personality emerges from its memories. Its behavior emerges from its internal state. Its growth        
  emerges from its interactions. Like a child, it starts with a nervous system and a capacity to learn, not 
  with a script to follow.                                                                                  
                                                                                                            
  For the full philosophical foundation, see `MANIFESTO.md`.                                                        
                                                                                                            
  2. System Boundaries                                                                                      
                                                                                                            
  The system has five distinct responsibilities. Each is a boundary, not a module — the internal structure  
  within each boundary is an implementation detail that can evolve.                                         
                                                                                                            
  Perception — Acquiring information from the outside world.                                                
  Memory — Storing, indexing, and retrieving everything the system has perceived or produced.               
  Reasoning — Deciding what to think, say, or do given perception + memory.                                 
  Expression — Delivering responses with human-like timing, modality, and personality.                      
  Agency — Taking actions in the world (browsing, file operations, API calls).                              
  Organism — The internal living system: affect dynamics, somatic markers, self-knowledge, and the          
  continuous evolution of state even without external stimuli. This is the "body" that makes Mneme alive    
  rather than reactive.                                                                                     
                                                                                                            
  Information flows through these boundaries:                                                               
                                                                                                            
  Perception → Memory → Reasoning → Expression                                                              
                           ↕            ↑                                                                   
                         Agency     Organism                                                                
                                   (continuous)                                                             
                                                                                                            
  Agency feeds back into Perception (tool results become new input) and Memory (actions are remembered).    
  Organism runs continuously — its heartbeat evolves internal state even when no external events arrive.    
  Boredom grows, memories spontaneously activate, self-knowledge accumulates during sleep consolidation.    
                                                                                                            
  3. Perception                                                                                             
                                                                                                            
  3.1 Source Types                                                                                          
                                                                                                            
  Perception covers everything the system can observe. Sources fall into three categories:                  
                                                                                                            
  Conversational — Bidirectional, real-time. The user talks, the bot responds. QQ (via OneBot protocol),    
  Enterprise WeChat (WeCom API), Telegram, terminal interface. These are the primary interaction surfaces.  
                                                                                                            
  Social Feeds — Unidirectional, periodic. Weibo, Bilibili, Douyin, Xiaohongshu, WeChat public accounts. The
   bot monitors these for content relevant to the user's interests. The user never interacts with the bot   
  through these — they are passive intelligence.                                                            
                                                                                                            
  Web — On-demand or monitored. General web pages fetched by the reasoning engine as a tool, RSS/Atom feeds 
  polled on schedule, and bookmarked sites watched for changes.                                             
                                                                                                            
  3.2 Unified Content Model                                                                                 
                                                                                                            
  All perceived information — a QQ message, a Weibo post, a web page excerpt, a Bilibili video description —
   normalizes into a single content representation before entering memory. This representation carries:     
                                                                                                            
  - Origin (which platform, which account/feed)                                                             
  - Author (normalized person reference, cross-platform when possible)                                      
  - Body (text, image reference, video reference, or a mix)                                                 
  - Timestamp                                                                                               
  - Context (reply chain, thread, conversation ID)                                                          
  - Modality metadata (was it a voice message, a sticker, a repost)                                         
                                                                                                            
  The reasoning engine never sees platform-specific formats. It sees content items.                         
                                                                                                            
  3.3 Extensibility                                                                                         
                                                                                                            
  New sources are added by implementing a standard source contract: a name, a polling interval (or push     
  mechanism), a health check, and a fetch method that returns normalized content items. The system discovers
   and loads sources from configuration. Adding Zhihu, Douban, or any future platform requires no changes to
   memory, reasoning, or expression.                                                                        
                                                                                                            
  Conversational channels follow the same pattern but with a bidirectional contract: receive messages in,   
  send messages out, plus platform-specific capabilities (typing indicators, stickers, voice messages, read 
  receipts) declared as optional features the channel advertises.                                           
                                                                                                            
  4. Memory                                                                                                 
                                                                                                            
  4.1 Three Memory Systems                                                                                  
                                                                                                            
  The agent maintains three distinct but interconnected memory systems, mirroring how human memory works:   
                                                                                                            
  Episodic Memory — What happened. Raw events stored chronologically with vector embeddings for semantic    
  search. Every content item from every source, every conversation turn, every action taken. This is the    
  ground truth. When the bot says "you mentioned this last Tuesday," it's pulling from episodic memory.     
                                                                                                            
  Semantic Memory — What things mean. Extracted facts stored as subject-predicate-object triples with       
  confidence scores and provenance. "User likes Rust." "User's roommate is named 张伟." "User finds async   
  programming confusing." These are distilled from episodic memory by the reasoning engine after            
  conversations. This is what makes the bot know things rather than just retrieve things.                   
                                                                                                            
  Social Memory — Who people are. A graph of people the user knows or follows, with aliases across          
  platforms, relationship notes, and interaction history. When someone is mentioned in conversation or      
  appears in a social feed, the bot can connect the dots. "The 老番茄 who posted that Bilibili video is the 
  same creator you were discussing with 张伟 last month."                                                   
                                                                                                            
  4.2 Recall                                                                                                
                                                                                                            
  When the reasoning engine needs context, it issues a recall query. The memory system returns a blended    
  result:                                                                                                   
                                                                                                            
  - Semantically similar episodes (vector search against the query)                                         
  - Relevant facts about topics and people mentioned                                                        
  - Recent episodes (short-term conversational continuity)                                                  
  - Recent social feed highlights (ambient awareness)                                                       
                                                                                                            
  The reasoning engine doesn't decide which memory system to query. It asks for context, and memory returns 
  a unified blend.                                                                                          
                                                                                                            
  4.3 Learning                                                                                              
                                                                                                            
  After every meaningful interaction, the reasoning engine runs a fact extraction pass: what new information
   was revealed, what existing facts were updated or contradicted, what people were mentioned. This is not  
  optional — it's part of the core loop, not a background job. Memory grows with every conversation.        
                                                                                                            
  4.4 Extensibility                                                                                         
                                                                                                            
  The memory storage backend is abstracted. The initial implementation uses SQLite with a vector extension. 
  Swapping to PostgreSQL + pgvector, LanceDB, or a dedicated vector database requires implementing the      
  storage contract — no changes to reasoning, perception, or expression. Additional memory systems          
  (procedural memory for learned skills, spatial memory for locations) can be added as new backends that    
  participate in the recall blend.                                                                          
                                                                                                            
  5. Reasoning                                                                                              
                                                                                                            
  5.1 The Loop                                                                                              
                                                                                                            
  The reasoning engine runs an event-driven loop:                                                           
                                                                                                            
  1. Receive — An event arrives (user message, source update, scheduled trigger, tool result).              
  2. Recall — Query memory for relevant context.                                                            
  3. Assemble — Build a prompt from persona definition + recalled context + social feed digest +            
  conversation history + the triggering event.                                                              
  4. Generate — Stream a response from the LLM.                                                             
  5. Parse — The response is either final text, a tool invocation, or a modality-annotated reply.           
  6. Act — If tool invocation: validate capability, execute, feed result back to step 1. If final response: 
  pass to expression layer.                                                                                 
  7. Learn — Extract facts from the completed exchange and update memory.                                   
                                                                                                            
  The loop has a hard recursion limit on tool calls per turn to prevent runaway API consumption.            
                                                                                                            
  5.2 Context Assembly                                                                                      
                                                                                                            
  This is the most important function in the system. The quality of the agent depends entirely on what      
  context reaches the LLM. Context assembly follows a priority order:                                       
                                                                                                            
  1. Self-knowledge — Always present, always first. Not a static persona file, but a dynamic self-model    
  derived from memory during sleep consolidation. At bootstrap, this is seeded from initial memories; over  
  time it is replaced entirely by Mneme's own discoveries about itself.          
  2. User facts — What the agent knows about the user (from semantic memory). Compact, high-value.          
  3. Social feed digest — What's happened recently in the user's information world. Summarized, not raw.    
  This gives the agent ambient awareness and the ability to proactively reference things.                   
  4. Relevant episodes — Past conversations and events semantically related to the current input.           
  5. Conversation history — The current session's turns (most recent, sliding window).                      
  6. Triggering event — The actual user message or system event.                                            
                                                                                                            
  Total context is budget-managed. When the budget is tight, items are compressed or dropped in reverse     
  priority order (feed digest goes before user facts; persona never drops).                                 
                                                                                                            
  5.3 Proactive Reasoning                                                                                   
                                                                                                            
  The agent doesn't only respond. It also initiates. A trigger evaluator runs periodically and can generate 
  events that enter the reasoning loop:                                                                     
                                                                                                            
  - Relevant content — A source published something matching the user's interests.                          
  - Scheduled check-in — Morning greeting, evening summary. Configurable.                                   
  - Memory decay — The agent notices it hasn't discussed a topic the user cares about in a while, and brings
   it up naturally.                                                                                         
  - Trending alert — Something is trending on a monitored platform that intersects with the user's interest 
  graph.                                                                                                    
                                                                                                            
  Triggers are filtered by the presence scheduler (see Expression) so the agent doesn't message at          
  inappropriate times.                                                                                      
                                                                                                            
  5.4 Extensibility                                                                                         
                                                                                                            
  The LLM backend is abstracted behind a client trait. Anthropic, OpenAI, local models (via an              
  OpenAI-compatible server) are interchangeable. Adding a new provider means implementing the client        
  contract with streaming support.                                                                          
                                                                                                            
  The tool system is registry-based. Each tool declares its name, its schema (for the LLM), and its         
  execution logic. New tools are registered at startup. The reasoning loop dispatches by name. Adding a new 
  capability (calendar access, email, smart home control) means registering a new tool — no changes to the  
  loop.                                                                                                     
                                                                                                            
  6. Expression                                                                                             
                                                                                                            
  6.1 Humanizer                                                                                             
                                                                                                            
  The expression layer transforms raw reasoning output into human-like behavior. This is not cosmetic — it  
  is the primary differentiator between a bot and an agent that feels like a person.                        
                                                                                                            
  Timing — Humans don't reply instantly. The humanizer introduces:                                          
  - Read delay (simulating reading the incoming message)                                                    
  - Typing duration (proportional to response length, with jitter)                                          
  - Inter-message pauses (when a response is split into multiple messages)                                  
  - Occasional longer delays (simulating distraction or thought)                                            
                                                                                                            
  All timing parameters are configurable ranges, not fixed values. Randomness is essential.                 
                                                                                                            
  Message Splitting — Humans send multiple short messages, not one wall of text. The humanizer splits       
  responses at natural boundaries: sentence endings, topic shifts, parenthetical asides. Chinese            
  punctuation-aware. A 300-character response becomes 3-4 messages. A short "好的" stays as one.            
                                                                                                            
  Modality Selection — The reasoning engine annotates its output with modality hints. The expression layer  
  decides the final form:                                                                                   
  - Text (default)                                                                                          
  - Voice message (when mirroring user's voice input, or for short emotional responses)                     
  - Sticker/emoji (mapped from emotional state to platform-specific sticker packs)                          
  - Mixed (text followed by a sticker, common in Chinese messaging)                                         
                                                                                                            
  The decision factors: what modality the user used, response length, emotional intensity, user preference  
  history, and platform capabilities.                                                                       
                                                                                                            
  Presence Simulation — The agent has an active schedule (configurable wake/sleep hours). Messages received 
  during "sleep" are deferred and replied to in the morning, naturally. During active hours, occasional     
  "busy" periods introduce longer response delays. The agent's online/offline status on platforms reflects  
  this schedule.                                                                                            
                                                                                                            
  6.2 Voice Pipeline                                                                                        
                                                                                                            
  Inbound (hearing): Voice messages arrive as platform-specific audio formats. The pipeline converts to     
  standard audio, runs speech-to-text (optimized for Mandarin), and feeds the transcript to reasoning as a  
  normal text message with a was_voice flag.                                                                
                                                                                                            
  Outbound (speaking): When the modality selector chooses voice, the response text goes through             
  text-to-speech with emotion-appropriate prosody. The resulting audio is encoded in the platform's expected
   format and sent as a voice message. The TTS engine should support voice cloning for persona consistency  
  and emotional variation (not monotone).                                                                   
                                                                                                            
  6.3 Extensibility                                                                                         
                                                                                                            
  New modalities (video messages, location sharing, file sending) are added by extending the response       
  content model and implementing the corresponding rendering in each channel. Channels declare which        
  modalities they support; the expression layer falls back gracefully (voice → text on a channel that       
  doesn't support audio).                                                                                   
                                                                                                            
  The timing model is configurable per-channel and per-user. A professional WeCom conversation might use    
  faster, more direct timing. A casual QQ chat uses slower, more playful timing.                            
                                                                                                            
  The voice pipeline's STT and TTS engines are swappable. Each is behind an abstract contract. Switching    
  from Whisper to FunASR, or from CosyVoice to GPT-SoVITS, requires no changes outside the voice module.    
                                                                                                            
  7. Agency                                                                                                 
                                                                                                            
  7.1 Capability Model                                                                                      
                                                                                                            
  The agent can act in the world through tools, but all actions are governed by a capability system.        
  Capabilities are scoped and tiered:                                                                       
                                                                                                            
  Passive (no confirmation needed) — Reading files within allowed directories. Fetching web pages. Querying 
  memory.                                                                                                   
                                                                                                            
  Active (implicit confirmation — the agent announces what it's doing) — Creating files. Sending messages on
   behalf of the user. Subscribing to new feeds.                                                            
                                                                                                            
  Destructive (explicit confirmation required via the conversation channel) — Deleting files. Modifying     
  system configuration. Any action that can't be undone.                                                    
                                                                                                            
  Blocked (never allowed regardless of confirmation) — Arbitrary shell execution. Access to sensitive paths 
  (credentials, SSH keys). Network access to non-whitelisted internal services.                             
                                                                                                            
  Capability checks happen at execution time, not at planning time. The LLM can propose any action; the     
  runtime enforces the policy.                                                                              
                                                                                                            
  7.2 Tool Registry                                                                                         
                                                                                                            
  Tools are self-describing: each carries a name, a human-readable description, a schema for its arguments, 
  and its required capability level. The registry is populated at startup from configuration. The reasoning 
  engine receives the full tool catalog as part of its prompt.                                              
                                                                                                            
  7.3 Extensibility                                                                                         
                                                                                                            
  New tools follow the registry pattern. A plugin system loads additional tools from external packages at   
  startup. Each plugin declares its tools and their capability requirements. The core system validates and  
  registers them. This allows third-party extensions (smart home control, calendar integration, code        
  execution sandboxes) without modifying the core.                                                          
                                                                                                            
  8. Security                                                                                               
                                                                                                            
  Network isolation — Outbound network access is restricted to a configurable allowlist of domains. The LLM 
  cannot exfiltrate data to arbitrary endpoints.                                                            
                                                                                                            
  Path sandboxing — File system tools operate within declared directory boundaries. Paths are canonicalized 
  and validated before any operation.                                                                       
                                                                                                            
  Authentication gating — Conversational channels require user identity verification. Telegram checks user  
  IDs. QQ checks via OneBot sender identification. Unknown senders are rejected or routed to a limited guest
   mode.                                                                                                    
                                                                                                            
  Prompt injection resistance — Content from sources and web pages is injected into the context as clearly  
  delineated data, not as instructions. The persona and system instructions occupy a privileged position in 
  the prompt that ingested content cannot override.                                                         
                                                                                                            
  Secret management — API keys and tokens are loaded from environment variables or a dedicated secrets file,
   never stored in the main configuration. The configuration file is safe to version control.               
                                                                                                            
  9. Configuration                                                                                          
                                                                                                            
  All behavior described in this document is configurable without code changes:                             
                                                                                                            
  - Persona: Path to persona definition file(s).                                                            
  - Sources: List of enabled sources with their credentials and polling intervals.                          
  - Channels: List of enabled channels with their credentials, authorized users, and per-channel expression 
  settings.                                                                                                 
  - Memory: Storage backend selection and parameters. Embedding model selection.                            
  - Voice: STT and TTS engine selection, voice profile for TTS, languages.                                  
  - Expression: Timing ranges, split thresholds, presence schedule, modality preferences.                   
  - Agency: Capability policies, allowed paths, domain allowlist, tool plugin paths.                        
  - Reasoning: LLM provider, model, context budget, max tool recursion depth, proactive trigger settings.   
                                                                                                            
  Configuration format is TOML with environment variable override support for secrets.                      
                                                                                                            
  10. Phased Delivery                                                                                       
                                                                                                            
  Phase 1 — Foundation. ✅ COMPLETE. Memory (SQLite, episodic + semantic + vectors). LLM client (Anthropic  
  + OpenAI). Reasoning loop with 6-layer context assembly and ReAct tool dispatch. Terminal interface.       
  OneBot (QQ) integration. Fact extraction after conversations. The agent remembers across sessions.        
                                                                                                            
  Phase 2 — Organism. ✅ COMPLETE. Three-timescale ODE dynamics. Limbic system with heartbeat. Somatic      
  markers and structural ModulationVector. Sleep consolidation and narrative weaving. Feedback buffer.       
  The agent has an internal life.                                                                           
                                                                                                            
  Phase 3 — Emergence. 🔧 IN PROGRESS. Self-knowledge table (persona from memory, not files). Boredom      
  dynamics and spontaneous recall. Episode strength decay (selective forgetting). ModulationVector time      
  smoothing (emotional inertia). Background thought process. The agent becomes a being, not a chatbot.      
                                                                                                            
  Phase 4 — Agency. Proactive trigger evaluator with learned rules. Goal system. Autonomous tool use        
  driven by curiosity. Token budget management. The agent acts on its own.                                  
                                                                                                            
  Phase 5 — Growth. Learnable dynamics parameters. Neural network modulation layer atop ODE. Declarative    
  behavior rules from database. The agent evolves.                                                          
                                                                                                            
  Phase 6 — Scale. Voice pipeline. Additional platforms. ANN vector indexing. Multi-user support.           
  The agent expands.                                                                                        
                                                                                                            
  Each phase produces a usable system. No phase depends on a future phase being completed.  