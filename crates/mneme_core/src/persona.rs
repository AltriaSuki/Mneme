use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;
use anyhow::Context;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Psyche {
    pub hippocampus: String,
    pub limbic: String,
    pub cortex: String,
    pub broca: String,
    pub occipital: String,
}

impl Psyche {
    pub async fn load<P: AsRef<Path>>(root: P) -> anyhow::Result<Self> {
        let root = root.as_ref();
        
        let hippocampus = read_file(root.join("hippocampus.md")).await?;
        let limbic = read_file(root.join("limbic.md")).await?;
        let cortex = read_file(root.join("cortex.md")).await?;
        let broca = read_file(root.join("broca.md")).await?;
        let occipital = read_file(root.join("occipital.md")).await?;

        Ok(Self {
            hippocampus,
            limbic,
            cortex,
            broca,
            occipital,
        })
    }

    pub fn format_context(&self) -> String {
        format!(
            "CRITICAL IDENTITY:\n\n{}\n\nEMOTION & CORE:\n\n{}\n\nLOGIC & SKILLS:\n\n{}\n\nVOICE:\n\n{}\n\nSENSING:\n\n{}",
            self.hippocampus, self.limbic, self.cortex, self.broca, self.occipital
        )
    }
}

async fn read_file<P: AsRef<Path>>(path: P) -> anyhow::Result<String> {
    match fs::read_to_string(&path).await {
        Ok(content) => Ok(content),
        Err(_) => Ok(String::new()), // Return empty string if file missing, rather than crashing
    }
}
