//! Owned module-provider subscription management.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use deskhalloumi_core::{
    ModuleUpdate,
    runtime::{ModuleProviderReceiver, ProviderSnapshot, TaskSpawner},
};
use iced::{Subscription, futures::SinkExt};
use tracing::{info, warn};

use crate::app::Message;
use crate::module_loader::ModuleSubscription;

#[derive(Clone)]
pub struct ManagedModuleProvider {
    pub name: String,
    pub instance_generation: u64,
    pub receiver: ModuleProviderReceiver,
}

/// Reload acceptance gate for queued Iced messages. Subscription identity also
/// includes the instance generation, but a message already queued by the old
/// stream may still arrive after the provider map has been replaced.
pub fn snapshot_matches_active_provider(
    active: &ManagedModuleProvider,
    snapshot: &ProviderSnapshot<ModuleUpdate>,
) -> bool {
    snapshot.belongs_to_instance(active.instance_generation)
}

#[derive(Clone)]
struct ModuleWatchSpec {
    name: String,
    instance_generation: u64,
    receiver: ModuleProviderReceiver,
}

impl Hash for ModuleWatchSpec {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name.hash(state);
        self.instance_generation.hash(state);
    }
}

fn watch_module_updates(
    spec: &ModuleWatchSpec,
) -> impl iced::futures::Stream<Item = Message> + use<> {
    use iced::stream;

    let name = spec.name.clone();
    let mut receiver = spec.receiver.clone();
    stream::channel(8, async move |mut output| {
        while let Some(snapshot) = receiver.changed().await {
            if output
                .send(Message::ModuleProviderState(name.clone(), snapshot))
                .await
                .is_err()
            {
                break;
            }
        }
    })
}

pub fn iced_subscription(provider: &ManagedModuleProvider) -> Subscription<Message> {
    Subscription::run_with(
        ModuleWatchSpec {
            name: provider.name.clone(),
            instance_generation: provider.instance_generation,
            receiver: provider.receiver.clone(),
        },
        watch_module_updates,
    )
}

/// Register every producer with the process supervisor and return its typed
/// Tokio watch receiver directly to the Iced adapter. No global registry or
/// polling mutex is involved.
pub fn initialize_module_subscriptions(
    module_subscriptions: Vec<ModuleSubscription>,
    spawner: &TaskSpawner,
) -> Result<HashMap<String, ManagedModuleProvider>, String> {
    info!(
        modules = module_subscriptions.len(),
        "registering typed module providers with runtime supervisor"
    );

    let mut providers = HashMap::new();
    for mut module in module_subscriptions {
        let name = module.name.clone();
        let receiver = module.subscription.receiver();
        let instance_generation = receiver.instance_generation();
        let producer = module
            .subscription
            .take_worker()
            .ok_or_else(|| format!("module '{name}' subscription has no producer"))?;
        let producer_name = format!("module:{name}:producer");
        let producer_token = spawner.cancellation_token();
        let producer_name_for_log = producer_name.clone();
        spawner
            .try_spawn(producer_name, async move {
                tokio::select! {
                    _ = producer_token.cancelled() => {
                        info!(module = %name, "module provider cancelled");
                    }
                    _ = producer => {
                        warn!(module = %name, task = %producer_name_for_log, "module provider stopped without process cancellation");
                    }
                }
            })
            .map_err(|error| format!("failed to supervise module provider: {error}"))?;
        providers.insert(
            module.name.clone(),
            ManagedModuleProvider {
                name: module.name,
                instance_generation,
                receiver,
            },
        );
    }
    Ok(providers)
}

#[cfg(test)]
mod tests {
    include!("subscription_manager_tests.rs");
}
