use iced::widget::text;
use iced::{Element, Length, Task};
use tracing::{info, Level};
use unilii_core::{Module, ModuleUpdate};

struct UniliiBar {
    modules: Vec<Box<dyn Module>>,
}

impl Default for UniliiBar {
    fn default() -> Self {
        tracing_subscriber::fmt().with_max_level(Level::INFO).init();
        info!("Starting unilii status bar");

        UniliiBar {
            modules: Vec::new(), // Box<dyn Module> storage
        }
    }
}

#[derive(Debug, Clone)]
enum Message {
    ModuleUpdate(usize, ModuleUpdate),
}

fn update(bar: &mut UniliiBar, message: Message) -> Task<Message> {
    match message {
        Message::ModuleUpdate(idx, update) => {
            if let Some(module) = bar.modules.get_mut(idx) {
                let _ = module.update(update);
            }
        }
    }
    Task::none()
}

fn view(_bar: &UniliiBar) -> Element<'_, Message> {
    text("unilii status bar")
        .width(Length::Fill)
        .height(Length::Shrink)
        .into()
}

fn main() -> iced::Result {
    iced::application("unilii", update, view)
        .window_size((800.0, 24.0))
        .position(iced::window::Position::Specific(iced::Point { x: 0.0, y: 0.0 }))
        .run()
}
