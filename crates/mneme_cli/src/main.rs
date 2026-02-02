use clap::Parser;
use mneme_core::{Event, Content, Modality, Reasoning, Psyche, Memory};
use mneme_memory::SqliteMemory;
use mneme_reasoning::ReasoningEngine;
use std::sync::Arc;
use std::io::{self, Write};
use tracing::{info, error};
use uuid::Uuid;
use std::time::{SystemTime, UNIX_EPOCH};
use mneme_perception::{SourceManager, rss::RssSource};



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
    #[arg(short, long, default_value = "claude-3-opus-20240229")]
    model: String,


}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    let engine = ReasoningEngine::new(psyche, memory.clone(), &args.model)?;

    println!("Mneme System Online. Type 'quit' to exit. Type 'sync' to fetch sources.");
    print!("> ");
    io::stdout().flush()?;

    let stdin = io::stdin();
    let mut input = String::new();

    loop {
        input.clear();
        stdin.read_line(&mut input)?;
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
                println!("\nMneme: {}\n", response);
            }
            Err(e) => {
                error!("Error thinking: {}", e);
                println!("\n[System Error]: {}\n", e);
            }
        }

        print!("> ");
        io::stdout().flush()?;
    }

    Ok(())
}
