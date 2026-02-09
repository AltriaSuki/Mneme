use clap::Parser;
use mneme_core::{Event, Content, Modality, Reasoning, SeedPersona, Memory};
use mneme_memory::{SqliteMemory, OrganismCoordinator, OrganismConfig};
use mneme_limbic::LimbicSystem;
use mneme_reasoning::ReasoningEngine;
use mneme_expression::{ScheduledTriggerEvaluator, PresenceScheduler, Humanizer, RuminationEvaluator};
use std::sync::Arc;
use tracing::{info, error};
use uuid::Uuid;

use mneme_perception::{SourceManager, rss::RssSource};

use mneme_core::ReasoningOutput;
use mneme_onebot::OneBotClient;
use std::env;
use mneme_reasoning::{llm::LlmClient, providers::{anthropic::AnthropicClient, openai::OpenAiClient}};

use rustyline::error::ReadlineError;
use rustyline::{DefaultEditor, Config, EditMode};

async fn print_response(response: &ReasoningOutput, humanizer: &Humanizer, prefix: Option<&str>) {
    println!(""); // Spacer
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
    println!(""); // Spacer
}



#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Add RSS feed URLs to monitor (can be used multiple times)
    #[arg(long)]
    rss: Vec<String>,

    /// Path to the memory database
    #[arg(short, long, default_value = "mneme.db")]
    db: String,

    /// Path to the persona directory
    #[arg(short, long, default_value = "persona")]
    persona: String,

    /// Model to use
    #[arg(short, long, env = "ANTHROPIC_MODEL", default_value = "claude-4-5-sonnet-20250929")]
    model: String,


}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load .env file if it exists
    dotenv::dotenv().ok();
    
    tracing_subscriber::fmt::init();
    let args = Args::parse();

    info!("Initializing Mneme...");
    
    // 1. Initialize Memory (before Psyche — Psyche reads from DB)
    info!("Connecting to Memory at {}...", args.db);
    let memory = Arc::new(SqliteMemory::new(&args.db).await?);

    // 2. Load Psyche (ADR-002: persona emerges from memory)
    info!("Loading seed persona from {}...", args.persona);
    let seed = SeedPersona::load(&args.persona).await?;
    if !seed.is_empty() {
        let seeded = memory.seed_self_knowledge(&seed.to_seed_entries()).await?;
        if seeded > 0 {
            info!("First run: seeded {} self-knowledge entries from persona files", seeded);
        }
    }
    let psyche = memory.build_psyche().await?;

    // 3. Initialize Source Manager
    let source_manager = Arc::new(SourceManager::new());
    for rss_url in args.rss {
        info!("Adding RSS source: {}", rss_url);
        // We use the URL as the name suffix for now to distinguish them
        let rss_source = Arc::new(RssSource::new(&rss_url, &rss_url)?);
        source_manager.add_source(rss_source).await;
    }

    // 4. Initialize Reasoning
    info!("Starting Reasoning Engine with model {}...", args.model);
    
    // Initialize OS Executor (Local for now, potentially SSH based on config later)
    use mneme_os::local::LocalExecutor;
    // Default to a safe timeout for agentic operations
    let executor = Arc::new(LocalExecutor::default());
    
    // Initialize LLM Client
    
    let provider = std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "anthropic".to_string());
    let client: Box<dyn LlmClient> = match provider.as_str() {
        "anthropic" => Box::new(AnthropicClient::new(&args.model)?),
        "openai" | "deepseek" | "codex" => Box::new(OpenAiClient::new(&args.model)?),
        _ => {
            tracing::warn!("Unknown provider '{}', defaulting to Anthropic", provider);
            Box::new(AnthropicClient::new(&args.model)?)
        },
    };

    // Initialize Organism Coordinator with persistence
    let limbic = Arc::new(LimbicSystem::new());
    let organism_config = OrganismConfig::default();
    let coordinator = Arc::new(
        OrganismCoordinator::with_persistence(limbic, organism_config, memory.clone()).await?
    );
    
    let mut engine = ReasoningEngine::with_coordinator(psyche, memory.clone(), client, executor, coordinator.clone());
    
    // 5. Initialize Proactive Triggers
    info!("Initializing proactive triggers...");
    // Add default scheduled triggers (morning/evening)
    let scheduler = ScheduledTriggerEvaluator::new();
    engine.add_evaluator(Box::new(scheduler));

    // Add rumination evaluator (mind-wandering, social longing, curiosity)
    let rumination = RuminationEvaluator::new(coordinator.state());
    engine.add_evaluator(Box::new(rumination));

    // Initialize Presence Scheduler (filters triggers by active hours)
    let presence = PresenceScheduler::default();
    info!("Presence scheduler active: {:?} - {:?}", presence.active_start, presence.active_end);

    // Initialize Humanizer for expressive output
    let humanizer = Humanizer::new();

    // Subscribe to lifecycle changes
    let mut lifecycle_rx = coordinator.subscribe_lifecycle();
    
    println!("Mneme System Online. Type 'quit' to exit. Type 'sync' to fetch sources. Type 'sleep' to trigger consolidation.");

    // --- ONEBOT MODE ---
    let onebot_url = env::var("ONEBOT_WS_URL").ok();
    
    // Channel for events from stdin (CLI) or OneBot
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<Event>(100);
    
    let onebot_client = if let Some(url) = onebot_url {
        let access_token = std::env::var("ONEBOT_ACCESS_TOKEN").ok();
        tracing::info!("Initializing OneBot at {}", url);
        match OneBotClient::new(&url, access_token.as_deref()) {
            Ok((client, mut onebot_rx)) => {
                // Forward OneBot content to main event loop
                let tx_clone = event_tx.clone();
                tokio::spawn(async move {
                    while let Some(content) = onebot_rx.recv().await {
                         let _ = tx_clone.send(Event::UserMessage(content)).await;
                    }
                });
                Some(Arc::<OneBotClient>::new(client))
            },
            Err(e) => {
                tracing::error!("Failed to init OneBot: {}", e);
                None
            }
        }
    } else {
        None
    };

    // Stdin loop using rustyline (line editing, history, CJK support)
    let tx_stdin = event_tx.clone();
    let coordinator_for_stdin = coordinator.clone();
    tokio::task::spawn_blocking(move || {
        // Set up rustyline with history
        let config = Config::builder()
            .edit_mode(EditMode::Emacs)
            .auto_add_history(true)
            .build();
        let mut rl = DefaultEditor::with_config(config).expect("Failed to init rustyline");
        
        // Load history from ~/.mneme_history
        let history_path = dirs::data_local_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("."))
            .join("mneme_history");
        let _ = rl.load_history(&history_path);
        
        loop {
            match rl.readline("> ") {
                Ok(line) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() { continue; }
                    
                    if trimmed == "quit" || trimmed == "exit" {
                        println!("Shutting down gracefully...");
                        // Use a tokio runtime handle to run async shutdown
                        let rt = tokio::runtime::Handle::current();
                        rt.block_on(coordinator_for_stdin.shutdown());
                        std::thread::sleep(std::time::Duration::from_millis(500));
                        // Save history before exit
                        let _ = rl.save_history(&history_path);
                        std::process::exit(0);
                    }
                    
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
                }
                Err(ReadlineError::Interrupted) => {
                    // Ctrl-C: graceful shutdown
                    println!("\nShutting down...");
                    let _ = rl.save_history(&history_path);
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(coordinator_for_stdin.shutdown());
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    std::process::exit(0);
                }
                Err(ReadlineError::Eof) => {
                    // Ctrl-D: exit
                    let _ = rl.save_history(&history_path);
                    break;
                }
                Err(err) => {
                    tracing::error!("Readline error: {:?}", err);
                    break;
                }
            }
        }
    });

    // --- MAIN EVENT LOOP ---
    // Now we listen to event_rx, which aggregates both stdin and OneBot
    // We also need a timer for triggering proactive evaluation and organism tick
    let mut trigger_interval = tokio::time::interval(std::time::Duration::from_secs(60)); // Check triggers every minute
    let mut tick_interval = tokio::time::interval(std::time::Duration::from_secs(10)); // Organism tick every 10 seconds

    // Clone coordinator for signal handler
    let coordinator_for_signal = coordinator.clone();
    
    loop {
        let event = tokio::select! {
            Some(evt) = event_rx.recv() => evt,
            _ = tokio::signal::ctrl_c() => {
                println!("\nReceived Ctrl+C, shutting down gracefully...");
                coordinator_for_signal.shutdown().await;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                std::process::exit(0);
            },
            _ = lifecycle_rx.changed() => {
                let state = *lifecycle_rx.borrow();
                info!("Lifecycle state changed: {:?}", state);
                continue;
            },
            _ = tick_interval.tick() => {
                // Organism periodic tick (state maintenance)
                if let Err(e) = coordinator.tick().await {
                    tracing::warn!("Organism tick error: {}", e);
                }
                continue;
            },
            _ = trigger_interval.tick() => {
                // Proactive Trigger Check
                match engine.evaluate_triggers().await {
                    Ok(triggers) => {
                        let active_triggers = presence.filter_triggers(triggers);
                        for trigger in active_triggers {
                            info!("Proactive trigger fired: {:?}", trigger);
                            // We do NOT print here directly to avoid double printing.
                            // We wrap it in an Event and sending it to the Engine.
                            // The response handling logic below will take care of printing.
                            // However, we are in a select! branch, not easily "sending" to self.
                            // We can just execute engine.think() here directly?
                            // Yes, that's what the previous code did, but clearer if we just set `event` variable?
                            // No, `event` is the result of select!.
                            // We'll process it right here.
                            
                            let trigger_event = Event::ProactiveTrigger(trigger);
                            match engine.think(trigger_event).await {
                                Ok(response) => {
                                    print_response(&response, &humanizer, Some("Proactive")).await;
                                    // TODO: Also send to OneBot if appropriate (e.g. to default user)
                                }
                                Err(e) => error!("Error processing trigger: {}", e),
                            }
                        }
                    }
                     Err(e) => error!("Error evaluating triggers: {}", e),
                }
                continue;
            },
            else => break, // Channel closed
        };

        // Handle specific CLI commands that need main-thread access (like 'sync')
        if let Event::UserMessage(ref content) = event {
             if content.source == "cli" && content.body.trim() == "sync" {
                info!("Syncing sources...");
                let items = source_manager.collect_all().await;
                println!("Fetched {} items.", items.len());
                for item in &items {
                    println!("- [{}] {}", item.source, item.body.lines().next().unwrap_or(""));
                    if let Err(e) = memory.memorize(item).await {
                        error!("Failed to memorize item {}: {}", item.id, e);
                    }
                }
                // Update engine's feed digest cache (Layer 3)
                engine.update_feed_digest(&items).await;
                continue; // Skip thinking
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
                            println!("Consolidation skipped: {}", result.skip_reason.unwrap_or_default());
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
                println!("=======================");
                continue;
             } else if content.source == "cli" && content.body.trim().starts_with("os-exec ") {
                let cmd = content.body.trim_start_matches("os-exec ").trim();
                info!("Executing OS command: '{}'", cmd);
                
                // Temp: use LocalExecutor for now
                // In Phase 3 integration, this will be selected based on config or intent
                use mneme_os::Executor;
                let executor = mneme_os::local::LocalExecutor::new();
                match executor.execute(cmd).await {
                    Ok(output) => {
                        println!("--- Output ---");
                        println!("{}", output.trim());
                        println!("--------------");
                    },
                    Err(e) => error!("Execution failed: {:?}", e),
                }
                continue;
             } else if content.source == "cli" && content.body.trim().starts_with("browser-test ") {
                let url = content.body.trim_start_matches("browser-test ").trim();
                info!("Testing Browser Navigation to: '{}'", url);
                
                // Test Browser Client
                use mneme_browser::BrowserClient;
                match BrowserClient::new(true) { // Headless = true
                    Ok(mut client) => {
                        if let Err(e) = client.launch() {
                            error!("Failed to launch browser: {}", e);
                        } else {
                            println!("Browser launched. Navigating...");
                            if let Err(e) = client.goto(url) {
                                error!("Failed to navigate: {}", e);
                            } else {
                                match client.get_title() {
                                    Ok(title) => println!("Page Title: {}", title),
                                    Err(e) => error!("Failed to get title: {}", e),
                                }
                            }
                        }
                    },
                    Err(e) => error!("Failed to init browser client: {}", e),
                }
                continue;
             }
        }
        
        // Log incoming messages
        match &event {
            Event::UserMessage(content) => {
                if content.source.starts_with("onebot") {
                    tracing::info!("Received OneBot message ({}) from {}: {}", content.source, content.author, content.body);
                }
            }
            _ => {}
        }
        
        match engine.think(event.clone()).await {
             Ok(response) => {
                 // Handle Output
                 if response.content.trim().is_empty() {
                     tracing::debug!("Mneme decided to stay silent.");
                     continue;
                 }

                 if let Event::UserMessage(input_content) = &event {
                     if input_content.source.starts_with("onebot") {
                         // Reply via OneBot
                         if let Some(client) = &onebot_client {
                             let parts = humanizer.split_response(&response.content);
                             for part in parts {
                                 let delay = humanizer.typing_delay(&part, Some(response.emotion));
                                 tokio::time::sleep(delay).await;
                                 
                                 // Check routing
                                 if let Some(group_str) = input_content.source.strip_prefix("onebot:group:") {
                                     if let Ok(group_id) = group_str.parse::<i64>() {
                                         // Reply to Group
                                         if let Err(e) = client.send_group_message(group_id, &part).await {
                                             error!("Failed to send OneBot Group message: {}", e);
                                         }
                                     }
                                 } else if let Ok(user_id) = input_content.author.parse::<i64>() {
                                     // Reply to Private
                                     if let Err(e) = client.send_private_message(user_id, &part).await {
                                         error!("Failed to send OneBot Private message: {}", e);
                                     }
                                 }
                             }
                         }
                     } else {
                         // Reply via CLI
                         print_response(&response, &humanizer, None).await;
                     }
                    }
                 }
             Err(e) => tracing::error!("Reasoning error: {}", e),
        }
    }

    Ok(())
}
