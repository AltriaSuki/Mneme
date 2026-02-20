use clap::Parser;
mod schedule_tool;
use mneme_core::config::{MnemeConfig, SharedConfig};
use mneme_core::{Content, Event, Memory, Modality, Reasoning, SeedPersona};
use mneme_expression::{
    AttentionGate, ConsciousnessGate, CuriosityTriggerEvaluator, HabitDetector, Humanizer,
    MetacognitionEvaluator, PresenceScheduler, RuminationEvaluator, ScheduledTriggerEvaluator,
    SocialTriggerEvaluator,
};
use mneme_limbic::LimbicSystem;
use mneme_memory::{OrganismConfig, OrganismCoordinator, SqliteMemory};
use mneme_reasoning::ReasoningEngine;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

use mneme_core::ReasoningOutput;
#[cfg(feature = "onebot")]
use mneme_onebot::OneBotClient;
use mneme_reasoning::{
    llm::LlmClient,
    providers::{anthropic::AnthropicClient, openai::OpenAiClient},
};

use rustyline::error::ReadlineError;
use rustyline::{Completer, Config, EditMode, Editor, Helper, Highlighter, Hinter, Validator};

/// Rustyline helper providing tab-completion for CLI commands.
#[derive(Completer, Helper, Highlighter, Hinter, Validator)]
struct MnemeHelper {
    #[rustyline(Completer)]
    completer: CommandCompleter,
}

#[derive(Clone)]
struct CommandCompleter;

impl rustyline::completion::Completer for CommandCompleter {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<String>)> {
        const COMMANDS: &[&str] = &[
            "quit", "exit", "status", "sleep", "like", "dislike", "reload",
        ];
        let prefix = &line[..pos];
        if prefix.contains(' ') {
            return Ok((0, vec![]));
        }
        let matches: Vec<String> = COMMANDS
            .iter()
            .filter(|cmd| cmd.starts_with(prefix))
            .map(|cmd| cmd.to_string())
            .collect();
        Ok((0, matches))
    }
}

/// Perform graceful shutdown with a 5-second timeout.
async fn graceful_shutdown(coordinator: &OrganismCoordinator) {
    let shutdown_fut = coordinator.shutdown();
    match tokio::time::timeout(std::time::Duration::from_secs(5), shutdown_fut).await {
        Ok(()) => tracing::info!("Graceful shutdown completed"),
        Err(_) => tracing::warn!("Shutdown timed out after 5s, forcing exit"),
    }
}

/// Format a compact CLI prompt showing organism status.
fn format_status_prompt(state: &mneme_core::OrganismState) -> String {
    let mood = if state.fast.affect.valence > 0.3 {
        "☀"
    } else if state.fast.affect.valence < -0.3 {
        "☁"
    } else {
        "·"
    };
    format!("[{}{:.0}%] > ", mood, state.fast.energy * 100.0)
}

async fn print_response(response: &ReasoningOutput, humanizer: &Humanizer, prefix: Option<&str>) {
    println!(); // Spacer
    let parts = humanizer.split_response(&response.content);
    for part in parts {
        // Simulate typing delay based on emotion
        // ReasoningOutput has implicit emotion in 'emotion' field, and we treat it as Option for Humanizer
        let delay = humanizer.typing_delay(&part, Some(response.emotion));
        tokio::time::sleep(delay).await;

        if let Some(p) = prefix {
            println!("[{}] Mneme: {}", p, part);
        } else {
            println!("Mneme: {}", part);
        }
    }
    println!(); // Spacer
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Path to TOML config file
    #[arg(short, long, default_value = "mneme.toml")]
    config: String,

    /// Path to the memory database (overrides config file)
    #[arg(long)]
    db: Option<String>,

    /// Path to the persona directory (overrides config file)
    #[arg(long)]
    persona: Option<String>,

    /// Model to use (overrides config file)
    #[arg(short, long)]
    model: Option<String>,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Output logs as JSON
    #[arg(long)]
    log_json: bool,

    /// Log file path (additional to stderr)
    #[arg(long)]
    log_file: Option<String>,

    /// OTLP endpoint for distributed tracing (requires --features otlp)
    #[arg(long, env = "OTEL_EXPORTER_OTLP_ENDPOINT")]
    otlp_endpoint: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if it exists
    dotenv::dotenv().ok();

    let args = Args::parse();

    // Configurable tracing subscriber
    {
        use tracing_subscriber::{fmt, prelude::*, EnvFilter};

        let env_filter =
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&args.log_level));

        // Optional OpenTelemetry layer (only available with --features otlp)
        #[cfg(feature = "otlp")]
        let otel_layer = {
            if let Some(ref endpoint) = args.otlp_endpoint {
                use opentelemetry::trace::TracerProvider;
                use opentelemetry_otlp::WithExportConfig;

                let exporter = opentelemetry_otlp::SpanExporter::builder()
                    .with_tonic()
                    .with_endpoint(endpoint)
                    .build()
                    .expect("Failed to create OTLP exporter");

                let provider = opentelemetry_sdk::trace::TracerProvider::builder()
                    .with_batch_exporter(exporter, opentelemetry_sdk::runtime::Tokio)
                    .build();

                let tracer = provider.tracer("mneme");
                opentelemetry::global::set_tracer_provider(provider);

                Some(tracing_opentelemetry::layer().with_tracer(tracer))
            } else {
                None
            }
        };
        #[cfg(not(feature = "otlp"))]
        let otel_layer: Option<tracing_subscriber::layer::Identity> = None;

        if let Some(ref log_path) = args.log_file {
            // File + stderr dual output
            let file_appender = tracing_appender::rolling::daily(
                std::path::Path::new(log_path)
                    .parent()
                    .unwrap_or(std::path::Path::new(".")),
                std::path::Path::new(log_path)
                    .file_name()
                    .unwrap_or(std::ffi::OsStr::new("mneme.log")),
            );
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
            // Leak the guard so it lives for the program's lifetime
            std::mem::forget(_guard);

            if args.log_json {
                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(otel_layer)
                    .with(fmt::layer().json().with_writer(std::io::stderr))
                    .with(fmt::layer().json().with_writer(non_blocking))
                    .init();
            } else {
                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(otel_layer)
                    .with(fmt::layer().with_writer(std::io::stderr))
                    .with(fmt::layer().with_writer(non_blocking))
                    .init();
            }
        } else if args.log_json {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(otel_layer)
                .with(fmt::layer().json())
                .init();
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(otel_layer)
                .with(fmt::layer())
                .init();
        }
    }

    // Load unified config (file + env overrides)
    let mut config = MnemeConfig::load_or_default(&args.config);

    // CLI args override config file
    if let Some(ref model) = args.model {
        config.llm.model = model.clone();
    }
    let db_path = args.db.as_deref().unwrap_or(&config.organism.db_path);
    let persona_dir = args
        .persona
        .as_deref()
        .unwrap_or(&config.organism.persona_dir);

    // Wrap config in arc-swap for hot reload
    let config_path = std::path::Path::new(&args.config);
    let shared_config = SharedConfig::new(
        config.clone(),
        if config_path.exists() {
            Some(config_path.to_path_buf())
        } else {
            None
        },
    );

    info!("Initializing Mneme...");

    // 1. Initialize Memory (before Psyche — Psyche reads from DB)
    info!("Connecting to Memory at {}...", db_path);
    let memory = Arc::new(SqliteMemory::new(db_path).await?);

    // 2. Load Psyche (ADR-002: persona emerges from memory)
    info!("Loading seed persona from {}...", persona_dir);
    let seed = SeedPersona::load(persona_dir).await?;
    if !seed.is_empty() {
        let seeded = memory.seed_self_knowledge(&seed.to_seed_entries()).await?;
        if seeded > 0 {
            info!(
                "First run: seeded {} self-knowledge entries from persona files",
                seeded
            );
        }
    }

    // Restart time gap awareness (#93)
    // Detect temporal discontinuity from process restarts so Mneme perceives
    // the gap rather than silently ignoring it.
    if let Ok(Some(last_ts)) = memory.last_episode_timestamp().await {
        let now_ts = chrono::Utc::now().timestamp();
        let gap_secs = now_ts - last_ts;
        if gap_secs > 1800 {
            // > 30 minutes gap — generate a discontinuity episode
            let gap_desc = if gap_secs > 86400 {
                format!("大约{}天", gap_secs / 86400)
            } else if gap_secs > 3600 {
                format!("大约{}小时", gap_secs / 3600)
            } else {
                format!("大约{}分钟", gap_secs / 60)
            };
            let episode = Content {
                id: Uuid::new_v4(),
                source: "self:restart".to_string(),
                author: "Mneme".to_string(),
                body: format!("我好像不在了一会儿……过去了{}。现在重新启动了。", gap_desc),
                timestamp: now_ts,
                modality: Modality::Text,
            };
            if let Err(e) = memory.memorize(&episode).await {
                error!("Failed to store restart episode: {}", e);
            } else {
                info!("Restart gap detected: {}s since last episode, stored discontinuity episode", gap_secs);
            }
        }
    }

    let psyche = memory.build_psyche(&config.organism.language).await?;

    // 3. Initialize Reasoning
    info!(
        "Starting Reasoning Engine with model {}...",
        config.llm.model
    );

    // Initialize LLM Client from config
    let timeout = config.llm.timeout_secs;
    let client: Box<dyn LlmClient> = match config.llm.provider.as_str() {
        "anthropic" => Box::new(AnthropicClient::new(&config.llm.model, timeout)?),
        "openai" | "deepseek" | "codex" => Box::new(OpenAiClient::new(&config.llm.model, timeout)?),
        "mock" => Box::new(mneme_reasoning::providers::mock::MockProvider::new(&config.llm.model)),
        _ => {
            tracing::warn!(
                "Unknown provider '{}', defaulting to Anthropic",
                config.llm.provider
            );
            Box::new(AnthropicClient::new(&config.llm.model, timeout)?)
        }
    };

    // Initialize Organism Coordinator with persistence
    let limbic = Arc::new(LimbicSystem::new());
    let organism_config = OrganismConfig::default();
    let coordinator = Arc::new(
        OrganismCoordinator::with_persistence(limbic, organism_config, memory.clone()).await?,
    );

    // v0.8.0: Wire up LLM Dream Narrator (Phase 2) — second client for dream generation
    {
        let dream_client: Arc<dyn LlmClient> = match config.llm.provider.as_str() {
            "anthropic" => Arc::new(AnthropicClient::new(&config.llm.model, timeout)?),
            "openai" | "deepseek" | "codex" => Arc::new(OpenAiClient::new(&config.llm.model, timeout)?),
            "mock" => Arc::new(mneme_reasoning::providers::mock::MockProvider::new(&config.llm.model)),
            _ => Arc::new(AnthropicClient::new(&config.llm.model, timeout)?),
        };
        let narrator = Arc::new(mneme_reasoning::LlmDreamNarrator::new(dream_client));
        coordinator.set_dream_narrator(narrator).await;
    }

    let mut engine = ReasoningEngine::with_coordinator(
        psyche,
        memory.clone(),
        client,
        coordinator.clone(),
    );

    // 4a. Wire up Social Graph (SqliteMemory implements SocialGraph)
    engine.set_social_graph(memory.clone());

    // 4a'. Context budget from config (linked to model's context window)
    engine.set_context_budget(config.llm.context_budget_chars);

    // 4b. Initialize Safety Guard
    let guard = Arc::new(mneme_core::safety::CapabilityGuard::new(
        config.safety.clone(),
    ));
    engine.set_guard(guard.clone());

    // 4c. Initialize Tool Registry + MCP tools
    let registry = {
        use mneme_reasoning::ToolRegistry;
        let mut registry = ToolRegistry::with_guard(guard);

        // Shell — the one hardcoded tool (her hands)
        registry.register(Box::new(mneme_reasoning::ShellToolHandler::new()));

        // Connect MCP servers and register discovered tools
        if let Some(ref mcp_config) = config.mcp {
            let lifecycle_rx = coordinator.subscribe_lifecycle();
            let mut mcp_manager =
                mneme_mcp::McpManager::new(mcp_config.servers.clone(), lifecycle_rx);
            match mcp_manager.connect_all().await {
                Ok(mcp_tools) => {
                    info!("MCP: {} tool(s) registered from MCP servers", mcp_tools.len());
                    for tool in mcp_tools {
                        registry.register(tool);
                    }
                }
                Err(e) => {
                    error!("MCP connection failed: {}", e);
                }
            }
            // Spawn lifecycle watcher (disconnects on sleep, etc.)
            mcp_manager.spawn_lifecycle_watcher();
        }

        Arc::new(tokio::sync::RwLock::new(registry))
    };
    engine.set_registry(registry.clone());

    // Track connected MCP server names for reload diffing
    let mut known_mcp_servers: std::collections::HashSet<String> = config
        .mcp
        .as_ref()
        .map(|m| m.servers.iter().filter(|s| s.auto_connect).map(|s| s.name.clone()).collect())
        .unwrap_or_default();

    // 4d. Initialize Token Budget
    let token_budget = Arc::new(mneme_reasoning::token_budget::TokenBudget::new(
        config.token_budget.clone(),
        memory.clone(),
    ));
    engine.set_token_budget(token_budget.clone());

    // 4e. Set streaming text callback for real-time output
    engine.set_on_text_chunk(Arc::new(|chunk: &str| {
        use std::io::Write;
        // Print text chunks as they arrive (no newline, flush immediately)
        print!("{}", chunk);
        let _ = std::io::stdout().flush();
    }));

    // 5. Initialize Proactive Triggers via AgentLoop
    info!("Initializing proactive triggers...");
    let scheduled_eval = ScheduledTriggerEvaluator::from_config(&config.organism.schedules);
    let schedule_handle = scheduled_eval.schedule_handle();

    // Register schedule self-editing tool
    {
        let mut reg = registry.write().await;
        reg.register(Box::new(schedule_tool::ScheduleToolHandler::new(schedule_handle.clone())));
    }

    let inner_evaluators: Vec<Box<dyn mneme_core::TriggerEvaluator>> = vec![
        Box::new(scheduled_eval),
        Box::new(RuminationEvaluator::new(coordinator.state())),
        Box::new(ConsciousnessGate::new(coordinator.state())),
        Box::new(MetacognitionEvaluator::new(
            coordinator.state(),
            coordinator.interaction_count_ref(),
        )),
        Box::new(HabitDetector::new(memory.clone())),
        Box::new(SocialTriggerEvaluator::new(coordinator.state(), memory.clone())),
        Box::new(CuriosityTriggerEvaluator::new(coordinator.state())),
    ];

    // B-17: Wrap all evaluators in AttentionGate for single-focus competition
    let attention_gate = AttentionGate::new(inner_evaluators);
    let engagement = attention_gate.engagement_handle();
    let evaluators: Vec<Box<dyn mneme_core::TriggerEvaluator>> = vec![Box::new(attention_gate)];

    // Initialize Presence Scheduler (filters triggers by active hours)
    let presence = PresenceScheduler::default();
    info!(
        "Presence scheduler active: {:?} - {:?}",
        presence.active_start, presence.active_end
    );

    // Initialize Humanizer for expressive output
    let humanizer = Humanizer::new();

    // 6. Spawn AgentLoop (background tick + trigger evaluation)
    let (agent_loop, mut agent_rx) = mneme_reasoning::agent_loop::AgentLoop::new(
        coordinator.clone(),
        evaluators,
        std::time::Duration::from_secs(config.organism.tick_interval_secs),
        std::time::Duration::from_secs(config.organism.trigger_interval_secs),
    );
    let _agent_handle = agent_loop.spawn();

    // Subscribe to lifecycle changes
    let mut lifecycle_rx = coordinator.subscribe_lifecycle();

    println!("Mneme System Online. Type 'quit' to exit. Type 'sync' to fetch sources. Type 'sleep' to trigger consolidation.");

    // Channel for events from stdin, OneBot, Gateway, etc.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<Event>(100);

    // --- ONEBOT MODE ---
    #[cfg(feature = "onebot")]
    let onebot_client = if let Some(ref onebot_cfg) = config.onebot {
        tracing::info!("Initializing OneBot at {}", onebot_cfg.ws_url);
        match OneBotClient::new(&onebot_cfg.ws_url, onebot_cfg.access_token.as_deref()) {
            Ok((client, mut onebot_rx)) => {
                // Forward OneBot content to main event loop
                let tx_clone = event_tx.clone();
                tokio::spawn(async move {
                    while let Some(content) = onebot_rx.recv().await {
                        let _ = tx_clone.send(Event::UserMessage(content)).await;
                    }
                });
                Some(Arc::<OneBotClient>::new(client))
            }
            Err(e) => {
                tracing::error!("Failed to init OneBot: {}", e);
                None
            }
        }
    } else {
        None
    };

    // --- GATEWAY MODE ---
    #[cfg(feature = "gateway")]
    let (gateway_response_tx, gateway_active_ws) = if let Some(ref gw_cfg) = config.gateway {
        let gw_server =
            mneme_gateway::GatewayServer::new(event_tx.clone(), &gw_cfg.host, gw_cfg.port);
        let resp_tx = gw_server.response_sender();
        let active_ws = gw_server.active_connections();
        gw_server.start();
        info!("Gateway started on {}:{}", gw_cfg.host, gw_cfg.port);
        (Some(resp_tx), Some(active_ws))
    } else {
        (None, None)
    };

    // Stdin loop using rustyline (line editing, history, CJK support)
    // Use a sync channel to block the stdin loop until the response is printed,
    // preventing the ">" prompt from appearing before Mneme's reply.
    // Shutdown is signaled via a oneshot channel instead of process::exit().
    let (prompt_ready_tx, prompt_ready_rx) = std::sync::mpsc::channel::<()>();
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let tx_stdin = event_tx.clone();

    // Shared prompt string — updated by main loop with organism status
    let cli_prompt = Arc::new(std::sync::Mutex::new(String::from("> ")));
    let cli_prompt_reader = cli_prompt.clone();

    tokio::task::spawn_blocking(move || {
        // Set up rustyline with history
        let config = Config::builder()
            .edit_mode(EditMode::Emacs)
            .auto_add_history(true)
            .build();
        let helper = MnemeHelper {
            completer: CommandCompleter,
        };
        let mut rl = Editor::with_config(config).expect("Failed to init rustyline");
        rl.set_helper(Some(helper));

        // Load history from ~/.mneme_history
        let history_path = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("mneme_history");
        let _ = rl.load_history(&history_path);

        let mut shutdown_tx = Some(shutdown_tx);

        loop {
            let prompt = cli_prompt_reader.lock().unwrap().clone();
            match rl.readline(&prompt) {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    if trimmed == "quit" || trimmed == "exit" {
                        println!("Shutting down gracefully...");
                        let _ = rl.save_history(&history_path);
                        if let Some(tx) = shutdown_tx.take() {
                            let _ = tx.send(());
                        }
                        return;
                    }

                    // Commands that don't need to wait for response
                    let needs_wait = !["sleep", "status", "like", "dislike", "reload"].contains(&trimmed);

                    let content = Content {
                        id: Uuid::new_v4(),
                        source: "cli".to_string(),
                        author: "User".to_string(),
                        body: line.clone(),
                        timestamp: chrono::Utc::now().timestamp(),
                        modality: Modality::Text,
                    };
                    if tx_stdin.blocking_send(Event::UserMessage(content)).is_err() {
                        break;
                    }

                    // Wait for the main loop to signal that the response has been printed
                    if needs_wait {
                        let _ = prompt_ready_rx.recv();
                    }
                }
                Err(ReadlineError::Interrupted) => {
                    // Ctrl-C: signal shutdown
                    println!("\nShutting down...");
                    let _ = rl.save_history(&history_path);
                    if let Some(tx) = shutdown_tx.take() {
                        let _ = tx.send(());
                    }
                    return;
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl-D: signal shutdown
                    let _ = rl.save_history(&history_path);
                    if let Some(tx) = shutdown_tx.take() {
                        let _ = tx.send(());
                    }
                    return;
                }
                Err(err) => {
                    tracing::error!("Readline error: {:?}", err);
                    if let Some(tx) = shutdown_tx.take() {
                        let _ = tx.send(());
                    }
                    return;
                }
            }
        }
    });

    // --- MAIN EVENT LOOP ---
    // Now we listen to event_rx (stdin + OneBot), agent_rx (AgentLoop actions),
    // and shutdown_rx (from readline thread quit/Ctrl-C/Ctrl-D/error).
    let mut shutdown_rx = shutdown_rx;

    loop {
        let event = tokio::select! {
            Some(evt) = event_rx.recv() => evt,
            _ = &mut shutdown_rx => {
                println!("Shutting down gracefully...");
                graceful_shutdown(&coordinator).await;
                break;
            },
            _ = tokio::signal::ctrl_c() => {
                println!("\nReceived Ctrl+C, shutting down gracefully...");
                graceful_shutdown(&coordinator).await;
                break;
            },
            _ = lifecycle_rx.changed() => {
                let state = *lifecycle_rx.borrow();
                info!("Lifecycle state changed: {:?}", state);
                continue;
            },
            Some(action) = agent_rx.recv() => {
                use mneme_reasoning::agent_loop::AgentAction;
                match action {
                    AgentAction::StateUpdate => {
                        // B-17: Decay engagement toward idle between interactions
                        engagement.decay(0.85);
                        continue;
                    }
                    AgentAction::ProactiveTrigger(trigger) => {
                        // Apply presence filter before processing
                        let active = presence.filter_triggers(vec![trigger]);
                        for t in active {
                            info!("Proactive trigger fired: {:?}", t);
                            let trigger_event = Event::ProactiveTrigger(t);
                            match engine.think(trigger_event).await {
                                Ok(response) if response.content.trim().is_empty() => {},
                                Ok(response) => {
                                    // Route based on response.route field
                                    #[allow(unused_mut, unused_assignments)]
                                    let mut routed = false;

                                    #[cfg(feature = "onebot")]
                                    if !routed {
                                        if let Some(ref route) = response.route {
                                            if let Some(ref client) = onebot_client {
                                                if let Some(gid_str) = route.strip_prefix("onebot:group:") {
                                                    if let Ok(gid) = gid_str.parse::<i64>() {
                                                        let parts = humanizer.split_response(&response.content);
                                                        for part in parts {
                                                            let delay = humanizer.typing_delay(&part, Some(response.emotion));
                                                            tokio::time::sleep(delay).await;
                                                            if let Err(e) = client.send_group_message(gid, &part).await {
                                                                error!("Proactive OneBot group send failed: {}", e);
                                                            }
                                                        }
                                                        routed = true;
                                                    }
                                                } else if let Some(uid_str) = route.strip_prefix("onebot:private:") {
                                                    if let Ok(uid) = uid_str.parse::<i64>() {
                                                        let parts = humanizer.split_response(&response.content);
                                                        for part in parts {
                                                            let delay = humanizer.typing_delay(&part, Some(response.emotion));
                                                            tokio::time::sleep(delay).await;
                                                            if let Err(e) = client.send_private_message(uid, &part).await {
                                                                error!("Proactive OneBot private send failed: {}", e);
                                                            }
                                                        }
                                                        routed = true;
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    if !routed {
                                        print_response(&response, &humanizer, Some("Proactive")).await;
                                    }
                                }
                                Err(e) => error!("Error processing trigger: {}", e),
                            }
                        }
                        continue;
                    }
                    AgentAction::AutonomousToolUse { tool_name, input, goal_id } => {
                        info!("Autonomous tool use: {} (goal={:?})", tool_name, goal_id);
                        match engine.execute_autonomous_tool(&tool_name, &input, goal_id).await {
                            Ok(result) => {
                                if !result.is_empty() {
                                    info!("Autonomous tool result: {}", &result[..result.len().min(200)]);
                                }
                            }
                            Err(e) => error!("Autonomous tool error: {}", e),
                        }
                        continue;
                    }
                }
            },
            else => break, // Channel closed
        };

        // Handle specific CLI commands that need main-thread access
        if let Event::UserMessage(ref content) = event {
            if content.source == "cli" && content.body.trim() == "reload" {
                match shared_config.reload() {
                    Ok(new_cfg) => {
                        engine.set_context_budget(new_cfg.llm.context_budget_chars);
                        // Hot-reload daily schedules
                        schedule_handle.reload(&new_cfg.organism.schedules);
                        // Connect newly-added MCP servers
                        if let Some(ref mcp) = new_cfg.mcp {
                            for server_cfg in &mcp.servers {
                                if !server_cfg.auto_connect || known_mcp_servers.contains(&server_cfg.name) {
                                    continue;
                                }
                                match mneme_mcp::McpManager::connect_one(server_cfg).await {
                                    Ok(tools) => {
                                        let count = tools.len();
                                        let mut reg = registry.write().await;
                                        for tool in tools {
                                            reg.register(tool);
                                        }
                                        known_mcp_servers.insert(server_cfg.name.clone());
                                        println!("MCP server '{}': {} tool(s) connected", server_cfg.name, count);
                                    }
                                    Err(e) => {
                                        error!("Failed to connect MCP server '{}': {}", server_cfg.name, e);
                                    }
                                }
                            }
                        }
                        println!(
                            "Config reloaded. context_budget={}, temperature={}, max_tokens={}",
                            new_cfg.llm.context_budget_chars,
                            new_cfg.llm.temperature,
                            new_cfg.llm.max_tokens,
                        );
                    }
                    Err(e) => println!("Reload failed: {}", e),
                }
                continue;
            } else if content.source == "cli" && content.body.trim() == "sleep" {
                // Manual sleep/consolidation trigger
                info!("Triggering sleep consolidation...");
                match coordinator.trigger_sleep().await {
                    Ok(result) => {
                        if result.performed {
                            println!("Sleep consolidation completed.");
                            if let Some(chapter) = result.new_chapter {
                                println!("New narrative chapter: {}", chapter.title);
                            }
                            if result.crisis.is_some() {
                                println!("⚠️ Narrative crisis detected during consolidation.");
                            }
                        } else {
                            println!(
                                "Consolidation skipped: {}",
                                result.skip_reason.unwrap_or_default()
                            );
                        }
                    }
                    Err(e) => error!("Sleep consolidation failed: {}", e),
                }
                continue;
            } else if content.source == "cli" && content.body.trim() == "status" {
                // Show organism status
                let state = coordinator.state().read().await.clone();
                let lifecycle = coordinator.lifecycle_state().await;
                println!("=== Organism Status ===");
                println!("Lifecycle: {:?}", lifecycle);
                println!("Energy: {:.2}", state.fast.energy);
                println!("Stress: {:.2}", state.fast.stress);
                println!("Mood bias: {:.2}", state.medium.mood_bias);
                println!("Affect: {}", state.fast.affect.describe());
                println!("Attachment: {:?}", state.medium.attachment.style());
                // Token usage
                let daily = token_budget.get_daily_usage().await;
                let monthly = token_budget.get_monthly_usage().await;
                println!("Token usage (today): {}", daily);
                println!("Token usage (month): {}", monthly);
                // Connection status
                #[cfg(feature = "onebot")]
                if let Some(ref client) = onebot_client {
                    let connected = if client.is_connected() { "Connected" } else { "Disconnected" };
                    let pending = client.pending_count();
                    if pending > 0 {
                        println!("OneBot: {} ({} pending)", connected, pending);
                    } else {
                        println!("OneBot: {}", connected);
                    }
                }
                #[cfg(feature = "gateway")]
                if let Some(ref ws_count) = gateway_active_ws {
                    println!("Gateway: {} active WebSocket(s)", ws_count.load(std::sync::atomic::Ordering::Relaxed));
                }
                println!("=======================");
                continue;
            } else if content.source == "cli" && content.body.trim() == "like" {
                coordinator
                    .record_feedback(
                        mneme_memory::SignalType::UserEmotionalFeedback,
                        "用户点赞（CLI命令）".to_string(),
                        0.75,
                        0.6,
                    )
                    .await;
                println!("👍 已记录");
                continue;
            } else if content.source == "cli" && content.body.trim() == "dislike" {
                coordinator
                    .record_feedback(
                        mneme_memory::SignalType::UserEmotionalFeedback,
                        "用户点踩（CLI命令）".to_string(),
                        0.8,
                        -0.6,
                    )
                    .await;
                println!("👎 已记录");
                continue;
            }
        }

        // Log incoming messages
        if let Event::UserMessage(content) = &event {
            if content.source.starts_with("onebot") {
                tracing::info!(
                    "Received OneBot message ({}) from {}: {}",
                    content.source,
                    content.author,
                    content.body
                );
            }
        }

        // B-17: Bump engagement on user interaction
        if matches!(&event, Event::UserMessage(_)) {
            engagement.set(1.0);
        }

        match engine.think(event.clone()).await {
            Ok(response) => {
                // Handle Output
                if response.content.trim().is_empty() {
                    tracing::debug!("Mneme decided to stay silent.");
                    // Update prompt with current state, then signal stdin loop
                    *cli_prompt.lock().unwrap() = format_status_prompt(&*coordinator.state().read().await);
                    let _ = prompt_ready_tx.send(());
                    continue;
                }

                #[allow(unused_variables)]
                if let Event::UserMessage(input_content) = &event {
                    #[allow(unused_mut, unused_assignments)]
                    let mut routed = false;

                    #[cfg(feature = "gateway")]
                    if !routed && input_content.source.contains("|req:") {
                        if let Some(ref gw_tx) = gateway_response_tx {
                            if let Some(req_str) = input_content.source.split("|req:").last() {
                                if let Ok(request_id) = req_str.parse::<Uuid>() {
                                    let gw_resp = mneme_gateway::GatewayResponse {
                                        request_id,
                                        content: response.content.clone(),
                                        emotion: Some(format!("{:?}", response.emotion)),
                                    };
                                    if let Err(e) = gw_tx.send(gw_resp).await {
                                        error!("Failed to send gateway response: {}", e);
                                    }
                                }
                            }
                        }
                        routed = true;
                    }

                    #[cfg(feature = "onebot")]
                    if !routed && input_content.source.starts_with("onebot") {
                        if let Some(client) = &onebot_client {
                            let parts = humanizer.split_response(&response.content);
                            for part in parts {
                                let delay = humanizer.typing_delay(&part, Some(response.emotion));
                                tokio::time::sleep(delay).await;
                                if let Some(group_str) =
                                    input_content.source.strip_prefix("onebot:group:")
                                {
                                    match group_str.parse::<i64>() {
                                        Ok(gid) => {
                                            if let Err(e) = client.send_group_message(gid, &part).await {
                                                error!("Failed to send OneBot Group message: {}", e);
                                            }
                                        }
                                        Err(e) => error!("OneBot: invalid group_id '{}': {}", group_str, e),
                                    }
                                } else {
                                    match input_content.author.parse::<i64>() {
                                        Ok(uid) => {
                                            if let Err(e) = client.send_private_message(uid, &part).await {
                                                error!("Failed to send OneBot Private message: {}", e);
                                            }
                                        }
                                        Err(e) => error!("OneBot: invalid user_id '{}': {}", input_content.author, e),
                                    }
                                }
                            }
                        }
                        routed = true;
                    }

                    if !routed {
                        // Reply via CLI (default)
                        print_response(&response, &humanizer, None).await;
                    }
                    // Update prompt with current state, then signal stdin loop
                    *cli_prompt.lock().unwrap() = format_status_prompt(&*coordinator.state().read().await);
                    let _ = prompt_ready_tx.send(());
                }
            }
            Err(e) => {
                tracing::error!("Reasoning error: {}", e);
                // Update prompt and signal stdin loop even on error
                *cli_prompt.lock().unwrap() = format_status_prompt(&*coordinator.state().read().await);
                let _ = prompt_ready_tx.send(());
            }
        }
    }

    // Flush any pending OTLP spans before exit
    #[cfg(feature = "otlp")]
    if args.otlp_endpoint.is_some() {
        opentelemetry::global::shutdown_tracer_provider();
    }

    Ok(())
}
