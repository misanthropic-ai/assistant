use ractor::{Actor, ActorRef, ActorProcessingErr};
use async_openai::{
    Client as OpenAIClient,
    config::OpenAIConfig,
    types::{
        ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
        ChatCompletionTool, ChatCompletionToolArgs, ChatCompletionToolType,
        ChatCompletionStreamResponseDelta, ChatCompletionResponseStream,
    },
};
use futures::{StreamExt, Stream};
use std::pin::Pin;
use tokio::sync::mpsc;
use crate::config::Config;
use crate::messages::{ChatMessage, ToolCall};
use uuid::Uuid;

/// Actor for OpenAI API communication
pub struct ClientActor {
    config: Config,
    client: OpenAIClient<OpenAIConfig>,
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
        messages: Vec<ChatCompletionRequestMessage>,
        tools: Vec<ChatCompletionTool>,
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
        tracing::info!("Client actor starting with model: {}", self.config.model);
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
                tracing::info!("Generating completion for request {}", id);
                
                // Cancel any existing stream
                if let Some(tx) = state.cancel_tx.take() {
                    let _ = tx.send(()).await;
                }
                if let Some(handle) = state.active_stream.take() {
                    handle.abort();
                }
                
                // Create cancellation channel
                let (cancel_tx, mut cancel_rx) = mpsc::channel::<()>(1);
                state.cancel_tx = Some(cancel_tx);
                
                // Create request
                let mut request_builder = CreateChatCompletionRequestArgs::default();
                request_builder
                    .model(&self.config.model)
                    .messages(messages)
                    .temperature(self.config.temperature)
                    .max_tokens(self.config.max_tokens as u32)
                    .stream(true);
                
                if !tools.is_empty() {
                    request_builder.tools(tools);
                }
                
                let request = request_builder.build()
                    .map_err(|e| format!("Failed to build request: {}", e))?;
                
                // Get stream
                let stream_result = self.client.chat().create_stream(request).await;
                
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
        // Create OpenAI config
        let mut openai_config = OpenAIConfig::new()
            .with_api_key(&config.api_key);
        
        // Set base URL if not default
        if config.base_url != "https://api.openai.com/v1" {
            openai_config = openai_config.with_api_base(&config.base_url);
        }
        
        let client = OpenAIClient::with_config(openai_config);
        
        Self {
            config,
            client,
        }
    }
    
    async fn handle_stream(
        mut stream: ChatCompletionResponseStream,
        chat_ref: Option<ActorRef<ChatMessage>>,
        request_id: Uuid,
        mut cancel_rx: mpsc::Receiver<()>,
    ) {
        let mut full_response = String::new();
        let mut pending_tool_calls: Vec<(usize, String, String, String)> = Vec::new();
        
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
                                        let index = tool_call.index as usize;
                                        
                                        // Ensure we have space for this index
                                        while pending_tool_calls.len() <= index {
                                            pending_tool_calls.push((index, String::new(), String::new(), String::new()));
                                        }
                                        
                                        if let Some(tc_id) = &tool_call.id {
                                            pending_tool_calls[index].1 = tc_id.clone();
                                        }
                                        
                                        if let Some(function) = &tool_call.function {
                                            if let Some(name) = &function.name {
                                                pending_tool_calls[index].2 = name.clone();
                                            }
                                            if let Some(args) = &function.arguments {
                                                pending_tool_calls[index].3.push_str(args);
                                            }
                                        }
                                    }
                                }
                                
                                // Check if stream is finished
                                if let Some(reason) = choice.finish_reason {
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
        
        // Send any pending tool calls
        for (_, id, name, args) in pending_tool_calls {
            if !name.is_empty() {
                if let Ok(parameters) = serde_json::from_str(&args) {
                    if let Some(ref chat_ref) = chat_ref {
                        let _ = chat_ref.send_message(ChatMessage::ToolRequest {
                            id: request_id,
                            call: ToolCall {
                                tool_name: name,
                                parameters,
                                delegate: false, // Will be determined by tool config
                            },
                        });
                    }
                }
            }
        }
        
        // Send completion
        if let Some(ref chat_ref) = chat_ref {
            let _ = chat_ref.send_message(ChatMessage::Complete {
                id: request_id,
                response: full_response,
            });
        }
    }
}