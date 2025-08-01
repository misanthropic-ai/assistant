use ractor::{Actor, ActorRef, ActorProcessingErr};
use futures::StreamExt;
use tokio::sync::mpsc;
use crate::config::Config;
use crate::messages::{ChatMessage, ToolCall};
use crate::openai_compat::{OpenAICompatClient, ChatCompletionRequest, ChatMessage as OpenAIMessage, Tool};
use uuid::Uuid;
use std::collections::HashMap;

/// Actor for OpenAI API communication
pub struct ClientActor {
    config: Config,
    client: OpenAICompatClient,
}

/// Client state tracking active streams
pub struct ClientState {
    active_stream: Option<tokio::task::JoinHandle<()>>,
    cancel_tx: Option<mpsc::Sender<()>>,
    chat_ref: Option<ActorRef<ChatMessage>>,
}

#[derive(Debug, Clone)]
pub enum ClientMessage {
    /// Set the chat actor reference
    SetChatRef(ActorRef<ChatMessage>),
    
    /// Generate completion
    Generate {
        id: Uuid,
        messages: Vec<OpenAIMessage>,
        tools: Vec<Tool>,
    },
    
    /// Cancel ongoing generation
    Cancel,
}

impl Actor for ClientActor {
    type Msg = ClientMessage;
    type State = ClientState;
    type Arguments = Config;
    
    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        _config: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Client actor starting");
        Ok(ClientState {
            active_stream: None,
            cancel_tx: None,
            chat_ref: None,
        })
    }
    
    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        msg: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match msg {
            ClientMessage::SetChatRef(chat_ref) => {
                tracing::debug!("Setting chat actor reference");
                state.chat_ref = Some(chat_ref);
            }
            
            ClientMessage::Generate { id, messages, tools } => {
                tracing::info!("Starting generation for request {}", id);
                
                // Cancel any existing stream
                if let Some(tx) = state.cancel_tx.take() {
                    let _ = tx.send(()).await;
                }
                if let Some(handle) = state.active_stream.take() {
                    handle.abort();
                }
                
                // Create cancellation channel
                let (cancel_tx, cancel_rx) = mpsc::channel::<()>(1);
                state.cancel_tx = Some(cancel_tx);
                
                // Create chat completion request
                let request = ChatCompletionRequest {
                    model: self.config.model.clone(),
                    messages,
                    tools: if tools.is_empty() { None } else { Some(tools) },
                    temperature: Some(self.config.temperature),
                    max_tokens: Some(self.config.max_tokens as u32),
                    stream: true,
                };

                if tracing::level_enabled!(tracing::Level::INFO) {
                    if let Ok(req_json) = serde_json::to_string_pretty(&request) {
                        tracing::info!("ChatCompletionRequest JSON:\n{}", req_json);
                    }
                }
                
                // Get stream
                let stream_result = self.client.create_chat_completion_stream(request).await;
                
                match stream_result {
                    Ok(stream) => {
                        let chat_ref = state.chat_ref.clone();
                        let request_id = id;
                        
                        // Spawn task to handle stream
                        let handle = tokio::spawn(async move {
                            Self::handle_stream(stream, chat_ref, request_id, cancel_rx).await;
                        });
                        
                        state.active_stream = Some(handle);
                    }
                    Err(e) => {
                        tracing::error!("Failed to create stream: {}", e);
                        if let Some(chat_ref) = &state.chat_ref {
                            let _ = chat_ref.send_message(ChatMessage::Error {
                                id,
                                error: format!("Failed to create stream: {}", e),
                            });
                        }
                    }
                }
            }
            
            ClientMessage::Cancel => {
                tracing::info!("Cancelling generation");
                
                if let Some(tx) = state.cancel_tx.take() {
                    let _ = tx.send(()).await;
                }
                if let Some(handle) = state.active_stream.take() {
                    handle.abort();
                }
            }
        }
        
        Ok(())
    }
}

impl ClientActor {
    pub fn new(config: Config) -> Self {
        let client = OpenAICompatClient::new(&config);
        
        Self {
            config,
            client,
        }
    }
    
    async fn handle_stream(
        mut stream: std::pin::Pin<Box<dyn futures::Stream<Item = Result<crate::openai_compat::ChatCompletionChunk, anyhow::Error>> + Send>>,
        chat_ref: Option<ActorRef<ChatMessage>>,
        request_id: Uuid,
        mut cancel_rx: mpsc::Receiver<()>,
    ) {
        let mut full_response = String::new();
        let mut pending_tool_calls: HashMap<usize, (String, String, String)> = HashMap::new();
        
        loop {
            tokio::select! {
                // Check for cancellation
                _ = cancel_rx.recv() => {
                    tracing::info!("Stream cancelled");
                    break;
                }
                
                // Process stream
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(response)) => {
                                if tracing::level_enabled!(tracing::Level::INFO) {
                                    tracing::info!("Received SSE chunk: {:?}", response);
                                }
                            for choice in response.choices {
                                let delta = &choice.delta;
                                // Handle content
                                if let Some(content) = &delta.content {
                                    full_response.push_str(content);
                                    
                                    if let Some(ref chat_ref) = chat_ref {
                                        let _ = chat_ref.send_message(ChatMessage::StreamToken {
                                            token: content.clone(),
                                        });
                                    }
                                }
                                
                                // Handle tool calls
                                if let Some(tool_calls) = &delta.tool_calls {
                                    for tool_call in tool_calls {
                                        // Handle negative indices from providers like OpenRouter
                                        let index = if tool_call.index < 0 {
                                            0
                                        } else {
                                            tool_call.index as usize
                                        };
                                        
                                        // Get or create entry for this index
                                        let entry = pending_tool_calls.entry(index)
                                            .or_insert_with(|| (String::new(), String::new(), String::new()));
                                        
                                        if let Some(tc_id) = &tool_call.id {
                                            entry.0 = tc_id.clone();
                                        }
                                        
                                        if let Some(function) = &tool_call.function {
                                            if let Some(name) = &function.name {
                                                entry.1 = name.clone();
                                            }
                                            if let Some(args) = &function.arguments {
                                                entry.2.push_str(args);
                                            }
                                        }
                                    }
                                }
                                
                                // Check if stream is finished
                                if let Some(reason) = &choice.finish_reason {
                                    tracing::debug!("Stream finished with reason: {:?}", reason);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            tracing::error!("Stream error: {}", e);
                            if let Some(ref chat_ref) = chat_ref {
                                let _ = chat_ref.send_message(ChatMessage::Error {
                                    id: request_id,
                                    error: format!("Stream error: {}", e),
                                });
                            }
                            break;
                        }
                        None => {
                            // Stream ended
                            break;
                        }
                    }
                }
            }
        }
        
        // Build tool calls from pending_tool_calls
        let mut tool_calls = Vec::new();
        for (_index, (id, name, args)) in pending_tool_calls {
            if !name.is_empty() && !id.is_empty() {
                if let Ok(parameters) = serde_json::from_str(&args) {
                    tool_calls.push(ToolCall {
                        tool_name: name,
                        parameters,
                        delegate: false, // Will be determined by tool config
                    });
                }
            }
        }
        
        // Send the assistant response with content and/or tool calls
        if !full_response.is_empty() || !tool_calls.is_empty() {
            if let Some(ref chat_ref) = chat_ref {
                tracing::info!("Sending AssistantResponse for request {} with content: {} and {} tool calls", 
                    request_id, 
                    if full_response.is_empty() { "<empty>" } else { &full_response },
                    tool_calls.len()
                );
                
                let _ = chat_ref.send_message(ChatMessage::AssistantResponse {
                    id: request_id,
                    content: if full_response.is_empty() { None } else { Some(full_response) },
                    tool_calls,
                });
            }
        }
    }
}