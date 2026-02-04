use clap::Parser;
use mneme_core::{Event, Content, Modality, Reasoning, Psyche, Memory};
use mneme_memory::SqliteMemory;
use mneme_reasoning::ReasoningEngine;
use mneme_expression::{ScheduledTriggerEvaluator, PresenceScheduler, Humanizer};
use std::sync::Arc;
use std::io::{self, Write};
use tracing::{info, error};
use uuid::Uuid;

use mneme_perception::{SourceManager, rss::RssSource};
use tokio::io::AsyncBufReadExt;

use mneme_core::ReasoningOutput;
use mneme_onebot::OneBotClient;
use std::env;

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
    
    // 1. Load Persona
    info!("Loading Psyche from {}...", args.persona);
    let psyche = Psyche::load(&args.persona).await?;
    
    // 2. Initialize Memory
    info!("Connecting to Memory at {}...", args.db);
    let memory = Arc::new(SqliteMemory::new(&args.db).await?);

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
    
    // Note: This will fail if ANTHROPIC_API_KEY is not set. 
    // For now, let's allow it to crash if key is missing to fail fast.
    let mut engine = ReasoningEngine::new(psyche, memory.clone(), &args.model, executor)?;
    
    // 5. Initialize Proactive Triggers
    info!("Initializing proactive triggers...");
    // Add default scheduled triggers (morning/evening)
    let scheduler = ScheduledTriggerEvaluator::new();
    engine.add_evaluator(Box::new(scheduler));

    // Initialize Presence Scheduler (filters triggers by active hours)
    let presence = PresenceScheduler::default();
    info!("Presence scheduler active: {:?} - {:?}", presence.active_start, presence.active_end);

    // Initialize Humanizer for expressive output
    let humanizer = Humanizer::new();

    println!("Mneme System Online. Type 'quit' to exit. Type 'sync' to fetch sources.");
    print!("> ");
    io::stdout().flush()?;

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

    // Stdin loop (always active for local debugging)
    let tx_stdin = event_tx.clone();
    tokio::spawn(async move {
        let stdin = tokio::io::stdin();
        let mut reader = tokio::io::BufReader::new(stdin).lines();
        while let Ok(Some(line)) = reader.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() { continue; }
            
            if trimmed == "quit" || trimmed == "exit" {
                std::process::exit(0);
            }
            
            let content = Content {
                id: Uuid::new_v4(),
                source: "cli".to_string(), 
                author: "User".to_string(),
                body: line, // Preserve whitespace for normal chat
                timestamp: chrono::Utc::now().timestamp(),
                modality: Modality::Text,
            };
            if let Err(_) = tx_stdin.send(Event::UserMessage(content)).await {
                break;
            }
        }
    });

    // --- MAIN EVENT LOOP ---
    // Now we listen to event_rx, which aggregates both stdin and OneBot
    // We also need a timer for triggering proactive evaluation
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60)); // Check triggers every minute

    loop {
        let event = tokio::select! {
            Some(evt) = event_rx.recv() => evt,
            _ = interval.tick() => {
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
                for item in items {
                    println!("- [{}] {}", item.source, item.body.lines().next().unwrap_or(""));
                    if let Err(e) = memory.memorize(&item).await {
                        error!("Failed to memorize item {}: {}", item.id, e);
                    }
                }
                continue; // Skip thinking
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
                print!("> ");
                io::stdout().flush()?;
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
                         print!("> ");
                         io::stdout().flush()?;
                     }
                    }
                 }
             Err(e) => tracing::error!("Reasoning error: {}", e),
        }
    }

    Ok(())
}
