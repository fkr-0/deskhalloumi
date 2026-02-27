use iced::widget::{container, row};
use iced::{Element, Length, Subscription, Task};
use std::collections::HashMap;
use tracing::{error, info, Level};
use unilii_core::ModuleUpdate;

mod module_loader;
use module_loader::{load_modules, LoadedModule};

struct UniliiBar {
    modules: HashMap<String, LoadedModule>,
}

impl Default for UniliiBar {
    fn default() -> Self {
        tracing_subscriber::fmt().with_max_level(Level::INFO).init();
        info!("Starting unilii status bar");

        // Modules will be loaded in run()
        UniliiBar {
            modules: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    ModuleUpdate(String, ModuleUpdate),
}

fn update(bar: &mut UniliiBar, message: Message) -> Task<Message> {
    match message {
        Message::ModuleUpdate(name, update) => {
            if let Some(loaded) = bar.modules.get_mut(&name) {
                if let Err(e) = loaded.module.update(update) {
                    error!("Failed to update module '{}': {}", name, e);
                }
            }
        }
    }
    Task::none()
}

fn view(bar: &UniliiBar) -> Element<'_, Message> {
    // Collect module views ordered by name
    let mut module_names: Vec<_> = bar.modules.keys().collect();
    module_names.sort();

    let mut right_widgets = vec![];

    for name in module_names {
        if let Some(loaded) = bar.modules.get(name) {
            let view = loaded.module.view();
            // Map module's internal ModuleUpdate messages to our Message
            let widget = view.map(move |update| Message::ModuleUpdate(name.clone(), update));

            right_widgets.push(widget);
        }
    }

    // Right section (clock, battery, etc.)
    let right_row = row(right_widgets)
        .spacing(4)
        .align_y(iced::Alignment::Center);

    // Create the status bar layout
    let bar_content = row![right_row]
        .spacing(8)
        .align_y(iced::Alignment::Center)
        .width(Length::Fill)
        .height(Length::Shrink);

    container(bar_content)
        .width(Length::Fill)
        .padding(4)
        .into()
}

fn subscribe(_bar: &UniliiBar) -> Subscription<Message> {
    // Note: Module updates are handled via tokio channels in module_loader
    Subscription::none()
}

#[tokio::main]
async fn main() -> iced::Result {
    // Load modules at startup
    let modules = load_modules().await.unwrap_or_else(|e| {
        error!("Failed to load modules: {}", e);
        HashMap::new()
    });

    info!("Loaded {} modules", modules.len());

    // Run the iced application with the loaded modules
    iced::application("unilii", update, view)
        .window_size((800.0, 24.0))
        .position(iced::window::Position::Specific(iced::Point {
            x: 0.0,
            y: 0.0,
        }))
        .subscription(subscribe)
        .run_with(|| (UniliiBar { modules }, Task::none()))
}
