//! Global subscription coordinator that bridges module subscriptions with Iced subscriptions.

use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use unilii_core::ModuleUpdate;

use crate::module_loader::ModuleSubscription;

/// Message from the global subscription system to Iced.
#[derive(Debug, Clone)]
pub enum GlobalMessage {
    ModuleUpdate(String, ModuleUpdate),
}

/// Global subscription coordinator that can be accessed without variable captures.
struct GlobalCoordinator {
    senders: Vec<mpsc::UnboundedSender<GlobalMessage>>,
    _subscription_tasks: Vec<tokio::task::JoinHandle<()>>,
}

static GLOBAL_COORDINATOR: Mutex<Option<GlobalCoordinator>> = Mutex::new(None);

fn coordinator_guard() -> std::sync::MutexGuard<'static, Option<GlobalCoordinator>> {
    match GLOBAL_COORDINATOR.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            warn!("Global coordinator mutex was poisoned, attempting recovery");
            poisoned.into_inner()
        }
    }
}

/// Register an Iced subscription channel to receive global messages.
pub fn register_subscription_channel() -> mpsc::UnboundedReceiver<GlobalMessage> {
    let (tx, rx) = mpsc::unbounded_channel();
    
    {
        let mut coordinator = coordinator_guard();
        if let Some(ref mut coord) = coordinator.as_mut() {
            coord.senders.push(tx);
        } else {
            *coordinator = Some(GlobalCoordinator {
                senders: vec![tx],
                _subscription_tasks: vec![],
            });
        }
    }
    
    rx
}

/// Initialize the global coordinator with module subscriptions.
pub fn initialize_global_coordinator(module_subscriptions: Vec<ModuleSubscription>) {
    let mut tasks = Vec::new();
    
    for mut sub in module_subscriptions {
        let name = sub.name.clone();
        let task = tokio::spawn(async move {
            info!("Starting global subscription handler for module: {}", name);
            
            while let Some(update) = sub.receiver.recv().await {
                let msg = GlobalMessage::ModuleUpdate(name.clone(), update.clone());
                broadcast_message(msg);
            }
            
            info!("Module subscription ended: {}", name);
        });
        
        tasks.push(task);
    }
    
    // Update the global coordinator with tasks
    {
        let mut coordinator = coordinator_guard();
        if let Some(ref mut coord) = coordinator.as_mut() {
            coord._subscription_tasks = tasks;
        }
    }
}

/// Broadcast a message to all registered Iced subscription channels.
fn broadcast_message(message: GlobalMessage) {
    let mut coordinator = coordinator_guard();
    if let Some(ref mut coord) = coordinator.as_mut() {
        // Keep only senders that are still connected
        coord.senders.retain(|sender| {
            match sender.send(message.clone()) {
                Ok(_) => true,
                Err(_) => {
                    // Remove disconnected senders
                    false
                }
            }
        });
    }
}

/// Create an Iced subscription that connects to the global message system.
/// This function doesn't capture any variables, making it compatible with Iced.
pub fn create_global_subscription() -> impl iced::futures::Stream<Item = GlobalMessage> {
    use tokio_stream::wrappers::UnboundedReceiverStream;
    
    let receiver = register_subscription_channel();
    UnboundedReceiverStream::new(receiver)
}