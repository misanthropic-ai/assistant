use ractor::{Actor, ActorRef, ActorProcessingErr};
use serde::{Deserialize, Serialize};
use reqwest::Client;
use scraper::{Html, Selector};
use std::time::Duration;
use crate::config::Config;
use crate::messages::{ToolMessage, ChatMessage};

/// Actor for performing web searches using DuckDuckGo
pub struct WebSearchActor {
    config: Config,
    client: Client,
}

/// WebSearch actor state
pub struct WebSearchState;

#[derive(Debug, Serialize, Deserialize)]
struct WebSearchParams {
    query: String,
    #[serde(default = "default_limit")]
    limit: usize,
}

#[derive(Debug)]
struct SearchResult {
    title: String,
    url: String,
    description: String,
}

fn default_limit() -> usize {
    5
}

impl Actor for WebSearchActor {
    type Msg = ToolMessage;
    type State = WebSearchState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::debug!("WebSearch actor starting");
        Ok(WebSearchState)
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ToolMessage::Execute { id, params, chat_ref } => {
                tracing::info!("Executing web search with params: {:?}", params);
                
                // Parse parameters
                let search_params: WebSearchParams = match serde_json::from_value(params) {
                    Ok(p) => p,
                    Err(e) => {
                        chat_ref.send_message(ChatMessage::ToolResult {
                            id,
                            result: format!("Error: Invalid parameters - {}", e),
                        })?;
                        return Ok(());
                    }
                };
                
                // Execute search
                let result = self.search(&search_params).await;
                
                // Send result back to chat
                chat_ref.send_message(ChatMessage::ToolResult {
                    id,
                    result,
                })?;
            }
            
            ToolMessage::Cancel { id } => {
                tracing::debug!("Cancelling web search operation {}", id);
                // Web search operations are typically quick and not cancellable
            }
            
            ToolMessage::StreamUpdate { .. } => {
                // WebSearch doesn't stream updates
            }
        }
        
        Ok(())
    }
}

impl WebSearchActor {
    pub fn new(config: Config) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
            .build()
            .unwrap_or_default();
            
        Self { config, client }
    }
    
    async fn search(&self, params: &WebSearchParams) -> String {
        // Validate query
        if params.query.trim().is_empty() {
            return String::from("Error: Search query cannot be empty");
        }
        
        // Build search URL
        let encoded_query = urlencoding::encode(&params.query);
        let url = format!("https://html.duckduckgo.com/html/?q={}", encoded_query);
        
        // Perform search
        match self.client.get(&url).send().await {
            Ok(response) => {
                match response.text().await {
                    Ok(html) => {
                        let results = self.parse_search_results(&html, params.limit);
                        self.format_results(&params.query, results)
                    }
                    Err(e) => {
                        format!("Error reading response: {}", e)
                    }
                }
            }
            Err(e) => {
                format!("Error performing search: {}", e)
            }
        }
    }
    
    fn parse_search_results(&self, html: &str, limit: usize) -> Vec<SearchResult> {
        let document = Html::parse_document(html);
        let mut results = Vec::new();
        
        // DuckDuckGo HTML results structure:
        // Results are in div with class "result results_links results_links_deep web-result"
        // Title is in h2 > a.result__a
        // URL is in a.result__a href attribute (needs extraction from redirect URL)
        // Description is in a.result__snippet
        
        let result_selector = Selector::parse("div.result.results_links.results_links_deep.web-result").unwrap();
        let title_selector = Selector::parse("h2 > a.result__a").unwrap();
        let snippet_selector = Selector::parse("a.result__snippet").unwrap();
        
        for result_element in document.select(&result_selector).take(limit) {
            let title_element = result_element.select(&title_selector).next();
            
            let title = title_element
                .as_ref()
                .map(|el| el.text().collect::<String>())
                .unwrap_or_default()
                .trim()
                .to_string();
            
            // Extract URL from the href attribute of the title link
            let url = title_element
                .and_then(|el| el.value().attr("href"))
                .and_then(|href| {
                    // DuckDuckGo wraps URLs in a redirect, extract the actual URL
                    if href.contains("uddg=") {
                        href.split("uddg=")
                            .nth(1)
                            .and_then(|u| u.split('&').next())
                            .and_then(|u| urlencoding::decode(u).ok())
                            .map(|u| u.into_owned())
                    } else {
                        Some(href.to_string())
                    }
                })
                .unwrap_or_default();
            
            let description = result_element
                .select(&snippet_selector)
                .next()
                .map(|el| el.text().collect::<String>())
                .unwrap_or_default()
                .trim()
                .to_string();
            
            if !title.is_empty() && !url.is_empty() {
                results.push(SearchResult {
                    title,
                    url,
                    description,
                });
            }
        }
        
        results
    }
    
    fn format_results(&self, query: &str, results: Vec<SearchResult>) -> String {
        if results.is_empty() {
            return format!("No results found for query: '{}'", query);
        }
        
        let mut output = format!("Search results for '{}':\n\n", query);
        
        for (i, result) in results.iter().enumerate() {
            output.push_str(&format!("{}. {}\n", i + 1, result.title));
            output.push_str(&format!("   URL: {}\n", result.url));
            if !result.description.is_empty() {
                output.push_str(&format!("   {}\n", result.description));
            }
            output.push('\n');
        }
        
        output.push_str(&format!("Total results shown: {}", results.len()));
        output
    }
}