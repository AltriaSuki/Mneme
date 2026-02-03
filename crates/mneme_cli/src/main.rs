use clap::Parser;
use mneme_core::{Event, Content, Modality, Reasoning, Psyche, Memory};
use mneme_memory::SqliteMemory;
use mneme_reasoning::ReasoningEngine;
use mneme_expression::{ScheduledTriggerEvaluator, PresenceScheduler};
use std::sync::Arc;
use std::io::{self, Write};
use tracing::{info, error};
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};
use mneme_perception::{SourceManager, rss::RssSource};
use tokio::io::AsyncBufReadExt;
use std::time::Duration;



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
    // Note: This will fail if ANTHROPIC_API_KEY is not set. 
    // For now, let's allow it to crash if key is missing to fail fast.
    let mut engine = ReasoningEngine::new(psyche, memory.clone(), &args.model)?;
    
    // 5. Initialize Proactive Triggers
    info!("Initializing proactive triggers...");
    // Add default scheduled triggers (morning/evening)
    let scheduler = ScheduledTriggerEvaluator::new();
    engine.add_evaluator(Box::new(scheduler));

    // Initialize Presence Scheduler (filters triggers by active hours)
    let presence = PresenceScheduler::default();
    info!("Presence scheduler active: {:?} - {:?}", presence.active_start, presence.active_end);

    println!("Mneme System Online. Type 'quit' to exit. Type 'sync' to fetch sources.");
    print!("> ");
    io::stdout().flush()?;

    // Use Async BufReader for stdin to allow select! to work
    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin).lines();
    
    // Trigger check interval (every 60 seconds)
    let mut interval = tokio::time::interval(Duration::from_secs(60));

    loop {
        tokio::select! {
            // 1. Periodic Trigger Check
            _ = interval.tick() => {
                match engine.evaluate_triggers().await {
                    Ok(triggers) => {
                        // Filter triggers based on presence (active hours)
                        let active_triggers = presence.filter_triggers(triggers);
                        
                        for trigger in active_triggers {
                            info!("Proactive trigger fired: {:?}", trigger);
                            let event = Event::ProactiveTrigger(trigger);
                            
                            // Note: proactive thinking might take time and block input reading
                            // In a future optimization, this could be spawned in a background task
                            match engine.think(event).await {
                                Ok(response) => {
                                    println!("\n[Proactive] Mneme: {}\n", response.content);
                                    print!("> ");
                                    io::stdout().flush()?;
                                }
                                Err(e) => error!("Error processing trigger: {}", e),
                            }
                        }
                    }
                    Err(e) => error!("Error evaluating triggers: {}", e),
                }
            }
            
            // 2. User Input
            line = reader.next_line() => {
                let input = match line {
                    Ok(Some(s)) => s,
                    Ok(None) => break, // EOF
                    Err(e) => {
                        error!("Error reading input: {}", e);
                        break;
                    }
                };

                let trimmed = input.trim();

                if trimmed == "quit" || trimmed == "exit" {
                    break;
                }

                if trimmed == "sync" {
                    info!("Syncing sources...");
                    let content = source_manager.collect_all().await;
                    println!("Fetched {} items.", content.len());
                    for item in content {
                        println!("- [{}] {}", item.source, item.body.lines().next().unwrap_or(""));
                        if let Err(e) = memory.memorize(&item).await {
                            error!("Failed to memorize item {}: {}", item.id, e);
                        }
                    }
                    print!("> ");
                    io::stdout().flush()?;
                    continue;
                }

                if trimmed.is_empty() {
                    print!("> ");
                    io::stdout().flush()?;
                    continue;
                }

                let event = Event::UserMessage(Content {
                    id: Uuid::new_v4(),
                    source: "terminal".to_string(),
                    author: "User".to_string(),
                    body: trimmed.to_string(),
                    timestamp: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs() as i64,
                    modality: Modality::Text,
                });

                match engine.think(event).await {
                    Ok(response) => {
                        println!("\nMneme: {}\n", response.content);
                    }
                    Err(e) => {
                        error!("Error thinking: {}", e);
                        println!("\n[System Error]: {}\n", e);
                    }
                }

                print!("> ");
                io::stdout().flush()?;
            }
        }
    }

    Ok(())
}
