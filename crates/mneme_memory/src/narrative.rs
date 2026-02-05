//! Narrative System - Life story weaving and identity management
//!
//! Based on Dan McAdams' narrative identity theory, this module:
//! - Periodically weaves episodic memories into autobiographical chapters
//! - Maintains a coherent life narrative across sessions
//! - Detects narrative crises and triggers identity updates
//!
//! The narrative is not just a summary - it's how Mneme understands herself.

use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc, Duration};
use std::collections::HashMap;

/// A chapter in Mneme's life narrative
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NarrativeChapter {
    /// Unique chapter ID
    pub id: i64,
    
    /// Chapter title (auto-generated or refined by LLM)
    pub title: String,
    
    /// The narrative text
    pub content: String,
    
    /// Time period covered
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    
    /// Dominant emotional tone (-1.0 to 1.0)
    pub emotional_tone: f32,
    
    /// Key themes identified
    pub themes: Vec<String>,
    
    /// Key people mentioned
    pub people_mentioned: Vec<String>,
    
    /// Turning points or significant events
    pub turning_points: Vec<TurningPoint>,
    
    /// When this chapter was written/updated
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A significant event that changes the narrative trajectory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurningPoint {
    /// Brief description
    pub description: String,
    
    /// Timestamp of the event
    pub timestamp: DateTime<Utc>,
    
    /// Impact on narrative direction (-1.0 to 1.0, negative = crisis)
    pub impact: f32,
    
    /// Values affected
    pub values_affected: Vec<String>,
}

/// Raw material for narrative weaving
#[derive(Debug, Clone)]
pub struct EpisodeDigest {
    pub timestamp: DateTime<Utc>,
    pub author: String,
    pub content: String,
    pub emotional_valence: f32,
}

/// The narrative weaver - creates autobiographical chapters from episodes
#[allow(dead_code)]
pub struct NarrativeWeaver {
    /// Minimum episodes needed to create a chapter
    min_episodes_per_chapter: usize,
    
    /// Maximum time span for a single chapter (reserved for future use)
    max_chapter_duration: Duration,
    
    /// Themes being tracked
    theme_keywords: HashMap<String, Vec<String>>,
}

impl Default for NarrativeWeaver {
    fn default() -> Self {
        Self::new()
    }
}

impl NarrativeWeaver {
    pub fn new() -> Self {
        let mut theme_keywords = HashMap::new();
        
        // Define theme detection keywords
        theme_keywords.insert("growth".to_string(), vec![
            "学习", "进步", "理解", "发现", "原来", "明白"
        ].into_iter().map(String::from).collect());
        
        theme_keywords.insert("connection".to_string(), vec![
            "朋友", "聊天", "分享", "一起", "喜欢", "关心"
        ].into_iter().map(String::from).collect());
        
        theme_keywords.insert("challenge".to_string(), vec![
            "困难", "问题", "挑战", "失败", "错误", "难过"
        ].into_iter().map(String::from).collect());
        
        theme_keywords.insert("joy".to_string(), vec![
            "开心", "高兴", "有趣", "好玩", "哈哈", "棒"
        ].into_iter().map(String::from).collect());
        
        theme_keywords.insert("reflection".to_string(), vec![
            "思考", "觉得", "认为", "其实", "或许", "意义"
        ].into_iter().map(String::from).collect());

        Self {
            min_episodes_per_chapter: 10,
            max_chapter_duration: Duration::days(7),
            theme_keywords,
        }
    }

    /// Weave episodes into a narrative chapter
    pub fn weave_chapter(
        &self,
        episodes: &[EpisodeDigest],
        chapter_id: i64,
    ) -> Option<NarrativeChapter> {
        if episodes.len() < self.min_episodes_per_chapter {
            return None;
        }

        let period_start = episodes.iter()
            .map(|e| e.timestamp)
            .min()?;
        let period_end = episodes.iter()
            .map(|e| e.timestamp)
            .max()?;

        // Detect themes
        let themes = self.detect_themes(episodes);
        
        // Calculate emotional tone
        let emotional_tone = episodes.iter()
            .map(|e| e.emotional_valence)
            .sum::<f32>() / episodes.len() as f32;

        // Extract people mentioned
        let people_mentioned = self.extract_people(episodes);

        // Detect turning points (significant emotional shifts)
        let turning_points = self.detect_turning_points(episodes);

        // Generate narrative content
        let content = self.generate_narrative_content(episodes, &themes, &turning_points);
        
        // Generate title
        let title = self.generate_title(&themes, emotional_tone, &period_start);

        let now = Utc::now();
        
        Some(NarrativeChapter {
            id: chapter_id,
            title,
            content,
            period_start,
            period_end,
            emotional_tone,
            themes,
            people_mentioned,
            turning_points,
            created_at: now,
            updated_at: now,
        })
    }

    /// Detect themes in episodes
    fn detect_themes(&self, episodes: &[EpisodeDigest]) -> Vec<String> {
        let mut theme_scores: HashMap<&str, u32> = HashMap::new();

        for episode in episodes {
            for (theme, keywords) in &self.theme_keywords {
                for keyword in keywords {
                    if episode.content.contains(keyword) {
                        *theme_scores.entry(theme.as_str()).or_default() += 1;
                    }
                }
            }
        }

        // Return themes that appear in at least 20% of episodes
        let threshold = (episodes.len() as f32 * 0.2) as u32;
        let mut themes: Vec<_> = theme_scores.into_iter()
            .filter(|(_, count)| *count >= threshold.max(1))
            .collect();
        
        themes.sort_by(|a, b| b.1.cmp(&a.1));
        themes.into_iter().map(|(t, _)| t.to_string()).take(3).collect()
    }

    /// Extract unique people mentioned
    fn extract_people(&self, episodes: &[EpisodeDigest]) -> Vec<String> {
        let mut people: Vec<String> = episodes.iter()
            .map(|e| e.author.clone())
            .collect();
        
        people.sort();
        people.dedup();
        people.retain(|p| p != "self" && p != "system");
        people
    }

    /// Detect emotional turning points
    fn detect_turning_points(&self, episodes: &[EpisodeDigest]) -> Vec<TurningPoint> {
        let mut turning_points = Vec::new();
        
        if episodes.len() < 3 {
            return turning_points;
        }

        // Use a sliding window to detect sudden emotional shifts
        let window_size = 3;
        for i in window_size..episodes.len() {
            let prev_avg = episodes[i-window_size..i].iter()
                .map(|e| e.emotional_valence)
                .sum::<f32>() / window_size as f32;
            
            let current = episodes[i].emotional_valence;
            let shift = current - prev_avg;

            // Significant shift detected
            if shift.abs() > 0.5 {
                turning_points.push(TurningPoint {
                    description: format!(
                        "情绪{}：{}",
                        if shift > 0.0 { "转好" } else { "转差" },
                        truncate_content(&episodes[i].content, 50)
                    ),
                    timestamp: episodes[i].timestamp,
                    impact: shift,
                    values_affected: vec![],
                });
            }
        }

        // Limit to top 3 most significant
        turning_points.sort_by(|a, b| b.impact.abs().partial_cmp(&a.impact.abs()).unwrap());
        turning_points.truncate(3);
        turning_points
    }

    /// Generate narrative content from episodes
    fn generate_narrative_content(
        &self,
        episodes: &[EpisodeDigest],
        themes: &[String],
        turning_points: &[TurningPoint],
    ) -> String {
        let mut content = String::new();

        // Opening based on themes
        if !themes.is_empty() {
            content.push_str(&format!(
                "这段时间的主题是{}。",
                themes.join("、")
            ));
        }

        // Episode count
        content.push_str(&format!(
            "共有{}次互动记录。",
            episodes.len()
        ));

        // Mention turning points
        if !turning_points.is_empty() {
            content.push_str("期间有一些重要时刻：");
            for tp in turning_points {
                content.push_str(&format!("{} ", tp.description));
            }
        }

        // Emotional summary
        let positive = episodes.iter().filter(|e| e.emotional_valence > 0.2).count();
        let negative = episodes.iter().filter(|e| e.emotional_valence < -0.2).count();
        let neutral = episodes.len() - positive - negative;

        content.push_str(&format!(
            "整体情绪分布：积极{}次，消极{}次，平淡{}次。",
            positive, negative, neutral
        ));

        content
    }

    /// Generate chapter title
    fn generate_title(&self, themes: &[String], tone: f32, start: &DateTime<Utc>) -> String {
        let time_desc = start.format("%Y年%m月").to_string();
        
        let tone_desc = if tone > 0.3 {
            "美好的"
        } else if tone < -0.3 {
            "艰难的"
        } else {
            "平静的"
        };

        let theme_desc = themes.first()
            .map(|t| match t.as_str() {
                "growth" => "成长",
                "connection" => "相遇",
                "challenge" => "挑战",
                "joy" => "欢乐",
                "reflection" => "思考",
                _ => "日常",
            })
            .unwrap_or("日常");

        format!("{}：{}的{}", time_desc, tone_desc, theme_desc)
    }

    /// Check if a narrative crisis is occurring (for triggering slow state updates)
    pub fn detect_crisis(&self, recent_episodes: &[EpisodeDigest], current_narrative_bias: f32) -> Option<CrisisEvent> {
        if recent_episodes.len() < 5 {
            return None;
        }

        // Calculate recent emotional average
        let recent_avg = recent_episodes.iter()
            .map(|e| e.emotional_valence)
            .sum::<f32>() / recent_episodes.len() as f32;

        // Significant deviation from narrative bias = crisis
        let deviation = (recent_avg - current_narrative_bias).abs();
        
        if deviation > 0.6 {
            return Some(CrisisEvent {
                description: format!(
                    "叙事偏差检测：近期情绪均值({:.2})与叙事倾向({:.2})严重不符",
                    recent_avg, current_narrative_bias
                ),
                intensity: deviation,
                timestamp: Utc::now(),
            });
        }

        // Check for value conflicts mentioned repeatedly
        let conflict_keywords = ["矛盾", "冲突", "不知道该", "两难", "纠结"];
        let conflict_count = recent_episodes.iter()
            .filter(|e| conflict_keywords.iter().any(|k| e.content.contains(k)))
            .count();

        if conflict_count >= 3 {
            return Some(CrisisEvent {
                description: "价值冲突：近期多次出现内心矛盾".to_string(),
                intensity: 0.5 + (conflict_count as f32 * 0.1),
                timestamp: Utc::now(),
            });
        }

        None
    }
}

/// A crisis event that may trigger narrative collapse
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrisisEvent {
    pub description: String,
    pub intensity: f32,
    pub timestamp: DateTime<Utc>,
}

/// Truncate content for display
fn truncate_content(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 3).collect();
        format!("{}...", truncated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_detection() {
        let weaver = NarrativeWeaver::new();
        
        let episodes: Vec<EpisodeDigest> = vec![
            EpisodeDigest {
                timestamp: Utc::now(),
                author: "user".to_string(),
                content: "今天学习了很多新东西".to_string(),
                emotional_valence: 0.5,
            },
            EpisodeDigest {
                timestamp: Utc::now(),
                author: "user".to_string(),
                content: "我发现了一个有趣的规律".to_string(),
                emotional_valence: 0.6,
            },
            EpisodeDigest {
                timestamp: Utc::now(),
                author: "user".to_string(),
                content: "终于明白这个概念了".to_string(),
                emotional_valence: 0.7,
            },
        ];

        let themes = weaver.detect_themes(&episodes);
        assert!(themes.contains(&"growth".to_string()));
    }

    #[test]
    fn test_turning_point_detection() {
        let weaver = NarrativeWeaver::new();
        
        let mut episodes = Vec::new();
        
        // Normal episodes
        for i in 0..5 {
            episodes.push(EpisodeDigest {
                timestamp: Utc::now() + Duration::minutes(i),
                author: "user".to_string(),
                content: "普通聊天".to_string(),
                emotional_valence: 0.0,
            });
        }
        
        // Sudden positive shift
        episodes.push(EpisodeDigest {
            timestamp: Utc::now() + Duration::minutes(6),
            author: "user".to_string(),
            content: "太开心了！".to_string(),
            emotional_valence: 0.9,
        });

        let turning_points = weaver.detect_turning_points(&episodes);
        assert!(!turning_points.is_empty());
        assert!(turning_points[0].impact > 0.0);
    }

    #[test]
    fn test_crisis_detection() {
        let weaver = NarrativeWeaver::new();
        
        // Episodes with very negative tone
        let episodes: Vec<EpisodeDigest> = (0..5).map(|i| EpisodeDigest {
            timestamp: Utc::now() + Duration::minutes(i as i64),
            author: "user".to_string(),
            content: "感觉很糟糕".to_string(),
            emotional_valence: -0.8,
        }).collect();

        // Current narrative bias is positive (mismatch!)
        let crisis = weaver.detect_crisis(&episodes, 0.5);
        assert!(crisis.is_some());
    }
}
