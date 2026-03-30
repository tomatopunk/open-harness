//! Debounce queue for batching memory updates (DeerFlow-inspired).

use crate::ids::{RunId, ThreadId};
use crate::thread_state::ChatMessage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

/// A conversation queued for memory processing.
#[derive(Debug, Clone)]
pub struct QueuedConversation {
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub messages: Vec<ChatMessage>,
    pub queued_at: Instant,
}

/// Result of processing a batch of conversations.
#[derive(Debug, Clone)]
pub struct MemoryUpdateBatch {
    pub thread_id: ThreadId,
    pub run_id: RunId,
    pub all_messages: Vec<ChatMessage>,
    pub conversation_count: usize,
}

/// Debounce queue for batching memory updates.
///
/// This queue collects conversations within a time window (debounce period),
/// then batches them together for efficient LLM-based memory extraction.
pub struct MemoryUpdateQueue {
    sender: Option<mpsc::Sender<QueuedConversation>>,
    worker_handle: Option<tokio::task::JoinHandle<()>>,
    is_shutdown: Arc<AtomicBool>,
}

impl MemoryUpdateQueue {
    /// Create a new debounce queue.
    ///
    /// # Arguments
    /// * `debounce_duration` - Time window for batching conversations
    /// * `processor` - Async callback invoked when batch is ready
    pub fn new<F, Fut>(debounce_duration: Duration, processor: F) -> Self
    where
        F: Fn(MemoryUpdateBatch) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
    {
        let (sender, mut receiver) = mpsc::channel::<QueuedConversation>(100);
        let is_shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = is_shutdown.clone();

        let worker_handle = tokio::spawn(async move {
            let mut batch: Vec<QueuedConversation> = Vec::new();
            let mut deadline: Option<Instant> = None;

            loop {
                tokio::select! {
                    Some(conv) = receiver.recv() => {
                        // Group by thread_id (simple approach: process each thread separately)
                        batch.push(conv);

                        // Set or reset deadline
                        if deadline.is_none() {
                            deadline = Some(Instant::now() + debounce_duration);
                        } else {
                            // Reset deadline for new conversations
                            deadline = Some(Instant::now() + debounce_duration);
                        }
                    }
                    _ = async {
                        if let Some(dl) = deadline {
                            tokio::time::sleep_until(dl).await;
                        }
                    }, if deadline.is_some() => {
                        // Deadline reached, process batch
                        if !batch.is_empty() {
                            // Group by thread_id and run_id
                            let mut grouped: std::collections::HashMap<(ThreadId, RunId), Vec<ChatMessage>> =
                                std::collections::HashMap::new();

                            for conv in batch.drain(..) {
                                let key = (conv.thread_id, conv.run_id);
                                grouped.entry(key).or_default().extend(conv.messages);
                            }

                            // Process each group
                            for ((thread_id, run_id), messages) in grouped {
                                let batch_result = MemoryUpdateBatch {
                                    thread_id,
                                    run_id,
                                    all_messages: messages,
                                    conversation_count: 1,
                                };
                                processor(batch_result).await;
                            }
                        }

                        deadline = None;
                    }
                    else => {
                        // Channel closed
                        break;
                    }
                }

                if shutdown_clone.load(Ordering::Relaxed) {
                    // Process remaining batch before shutdown
                    if !batch.is_empty() {
                        let mut grouped: std::collections::HashMap<
                            (ThreadId, RunId),
                            Vec<ChatMessage>,
                        > = std::collections::HashMap::new();

                        for conv in batch.drain(..) {
                            let key = (conv.thread_id, conv.run_id);
                            grouped.entry(key).or_default().extend(conv.messages);
                        }

                        for ((thread_id, run_id), messages) in grouped {
                            let batch_result = MemoryUpdateBatch {
                                thread_id,
                                run_id,
                                all_messages: messages,
                                conversation_count: 1,
                            };
                            processor(batch_result).await;
                        }
                    }
                    break;
                }
            }
        });

        Self { sender: Some(sender), worker_handle: Some(worker_handle), is_shutdown }
    }

    /// Add a conversation to the queue.
    pub async fn add(
        &self,
        thread_id: ThreadId,
        run_id: RunId,
        messages: Vec<ChatMessage>,
    ) -> Result<(), mpsc::error::SendError<QueuedConversation>> {
        let sender = self.sender.as_ref().ok_or_else(|| {
            mpsc::error::SendError(QueuedConversation {
                thread_id,
                run_id,
                messages: messages.clone(),
                queued_at: Instant::now(),
            })
        })?;

        let conv = QueuedConversation { thread_id, run_id, messages, queued_at: Instant::now() };
        sender.send(conv).await
    }

    /// Shutdown the queue and process remaining items.
    pub async fn shutdown(mut self) {
        self.is_shutdown.store(true, Ordering::Relaxed);
        drop(self.sender.take());

        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.await;
        }
    }

    /// Get the number of items currently queued (approximate).
    #[must_use]
    pub fn queue_len(&self) -> usize {
        self.sender.as_ref().map(|s| s.max_capacity() - s.capacity()).unwrap_or(0)
    }
}

impl Drop for MemoryUpdateQueue {
    fn drop(&mut self) {
        self.is_shutdown.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_queue_creation() {
        let queue: MemoryUpdateQueue =
            MemoryUpdateQueue::new(Duration::from_millis(100), |_| async {});

        assert_eq!(queue.queue_len(), 0);
    }

    #[tokio::test]
    async fn test_queue_add_and_process() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let queue = MemoryUpdateQueue::new(Duration::from_millis(50), move |batch| {
            let received = received_clone.clone();
            async move {
                let mut guard = received.lock().await;
                guard.push(batch);
            }
        });

        let thread_id = ThreadId::new_v4();
        let run_id = RunId::new_v4();
        let messages =
            vec![ChatMessage { role: "user".to_string(), content: serde_json::json!("Hello") }];

        queue.add(thread_id, run_id, messages).await.unwrap();

        // Wait for debounce period
        tokio::time::sleep(Duration::from_millis(100)).await;

        let guard = received.lock().await;
        assert!(!guard.is_empty());
        assert_eq!(guard[0].thread_id, thread_id);
        assert_eq!(guard[0].run_id, run_id);
    }

    #[tokio::test]
    async fn test_queue_shutdown() {
        let received = Arc::new(Mutex::new(Vec::new()));
        let received_clone = received.clone();

        let queue = MemoryUpdateQueue::new(
            Duration::from_secs(10), // Long debounce
            move |batch| {
                let received = received_clone.clone();
                async move {
                    let mut guard = received.lock().await;
                    guard.push(batch);
                }
            },
        );

        let thread_id = ThreadId::new_v4();
        let run_id = RunId::new_v4();
        let messages =
            vec![ChatMessage { role: "user".to_string(), content: serde_json::json!("Test") }];

        queue.add(thread_id, run_id, messages).await.unwrap();
        queue.shutdown().await;

        // Should have processed remaining batch on shutdown
        let guard = received.lock().await;
        assert!(!guard.is_empty());
    }
}
