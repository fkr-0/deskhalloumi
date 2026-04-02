// Enhanced update function for the new tray system
fn enhanced_update(bar: &mut UniliiPanel, message: Message) -> Task<Message> {
    match message {
        Message::ModuleUpdate(name, update) => {
            info!("module update: {name} -> {:?}", update);
            if let Some(loaded) = bar.modules.get_mut(&name) {
                if let Err(e) = loaded.module.update(update) {
                    error!("Failed to update module '{}': {}", name, e);
                }
            }
        }
        Message::KeyboardInput { code, value } => {
            info!("keyboard event: code={code}, value={value}");
            if code == "KEY_LEFTSHIFT" || code == "KEY_RIGHTSHIFT" {
                bar.shift_held = value != 0;
                info!("shift state changed: held={}", bar.shift_held);
            }
            info!("evdev key: {code} ({value})");
        }
        Message::WindowKeyboardInput {
            key,
            pressed,
            is_shift,
        } => {
            if is_shift {
                bar.shift_held = pressed;
            }

            if pressed {
                // Enhanced menu keyboard navigation
                if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                    match key.as_str() {
                        "Named(Escape)" => {
                            tray_state.animation_target = 0.0;
                            return Task::none();
                        }
                        "Named(ArrowDown)" | "Named(Tab)" => {
                            let count = get_current_menu_item_count(&tray_state.current_view);
                            if count > 0 {
                                tray_state.selected_index = Some(match tray_state.selected_index {
                                    None => 0,
                                    Some(i) => (i + 1) % count,
                                });
                            }
                            return Task::none();
                        }
                        "Named(ArrowUp)" => {
                            let count = get_current_menu_item_count(&tray_state.current_view);
                            if count > 0 {
                                tray_state.selected_index = Some(match tray_state.selected_index {
                                    None => count.saturating_sub(1),
                                    Some(i) => if i == 0 { count - 1 } else { i - 1 },
                                });
                            }
                            return Task::none();
                        }
                        "Named(ArrowLeft)" => {
                            return Task::done(Message::TrayNavigateLeft);
                        }
                        "Named(ArrowRight)" => {
                            return Task::done(Message::TrayNavigateRight);
                        }
                        "Named(Enter)" => {
                            if let Some(idx) = tray_state.selected_index {
                                if let Some((app_id, action)) = get_menu_action_at_index(&tray_state.current_view, idx) {
                                    tray_state.animation_target = 0.0;
                                    return Task::done(Message::TrayMenuTriggered(app_id, action));
                                }
                            }
                            return Task::none();
                        }
                        "Character(f)" => {
                            return Task::done(Message::TrayToggleFavorite("".to_string(), "".to_string()));
                        }
                        "Character(a)" => {
                            return Task::done(Message::TrayShowAggregated);
                        }
                        "Character(v)" => {
                            return Task::done(Message::TrayShowFavorites);
                        }
                        _ => {}
                    }
                }

                // Shift + digit: open nth tray icon
                if bar.shift_held {
                    if let Some(idx) = key_char_digit(&key) {
                        if let Some(icon) = bar.tray_icons.get(idx) {
                            let icon_key = icon.key.clone();
                            return Task::done(Message::TrayIconPressed(icon_key));
                        }
                    }
                }
            }
        }

        // Enhanced tray events
        Message::EnhancedTrayEvent(event) => {
            match event {
                enhanced_tray::TrayEvent::Icons(icons) => {
                    bar.tray_icons = icons.clone();
                    
                    // Update enhanced tray state
                    if let Some(tray_state) = &mut bar.enhanced_tray {
                        // Update the tree with new/changed icons
                        for icon in icons {
                            tray_state.tree.update_app(icon);
                        }
                    } else {
                        // Initialize enhanced tray state
                        let mut tree = enhanced_tray::TrayMenuTree::new();
                        for icon in icons {
                            tree.update_app(icon);
                        }
                        bar.enhanced_tray = Some(EnhancedTrayState {
                            tree,
                            current_view: TrayViewState::Aggregated { items: vec![], filter: None },
                            animation_progress: 0.0,
                            animation_target: 0.0,
                            selected_index: None,
                            filter_text: String::new(),
                        });
                    }
                }
                enhanced_tray::TrayEvent::MenuUpdated { app_id, menu } => {
                    if let Some(tray_state) = &mut bar.enhanced_tray {
                        if let Some(app) = tray_state.tree.apps.get_mut(&app_id) {
                            app.menu_items = menu;
                            app.last_updated = std::time::SystemTime::now();
                        }
                    }
                }
                enhanced_tray::TrayEvent::FavoritesChanged(favorites) => {
                    if let Some(tray_state) = &mut bar.enhanced_tray {
                        tray_state.tree.favorites = favorites;
                    }
                }
                enhanced_tray::TrayEvent::DbusMenuReceived { app_id, menu } => {
                    if let Some(tray_state) = &mut bar.enhanced_tray {
                        let tray_menu = enhanced_tray::convert_dbus_to_tray_menu(menu, &app_id);
                        if let Some(app) = tray_state.tree.apps.get_mut(&app_id) {
                            app.menu_items = tray_menu;
                            app.last_updated = std::time::SystemTime::now();
                        }
                    }
                }
            }
        }

        Message::TrayIconPressed(icon_key) => {
            if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.key == icon_key) {
                // Initialize or update enhanced tray state
                let mut tree = enhanced_tray::TrayMenuTree::new();
                for icon in &bar.tray_icons {
                    tree.update_app(icon.clone());
                }

                // Determine view mode based on icon type
                let view_state = if !bar.run_options.no_network_menu && enhanced_tray::is_network_icon(icon) {
                    TrayViewState::Network {
                        app_id: icon.id.clone(),
                        data: None,
                        loading: true,
                        error: None,
                    }
                } else {
                    let navigation = tree.get_app_navigation(&icon.id);
                    TrayViewState::SingleApp {
                        app_id: icon.id.clone(),
                        navigation,
                    }
                };

                bar.enhanced_tray = Some(EnhancedTrayState {
                    tree,
                    current_view: view_state,
                    animation_progress: 0.0,
                    animation_target: 1.0,
                    selected_index: Some(0),
                    filter_text: String::new(),
                });

                // If network icon, fetch network data
                if !bar.run_options.no_network_menu && enhanced_tray::is_network_icon(icon) {
                    let nmcli_path = bar.run_options.nmcli_path.clone();
                    let app_id = icon.id.clone();
                    return Task::perform(
                        enhanced_tray::read_network_snapshot(nmcli_path, false),
                        move |result| Message::TrayNetworkSnapshot(app_id, result),
                    );
                }
            }
        }

        Message::TrayMenuTriggered(app_id, action) => {
            if let Some(icon) = bar.tray_icons.iter().find(|icon| icon.id == app_id).cloned() {
                tokio::spawn(async move {
                    enhanced_tray::invoke_menu_action(&icon, action).await;
                });
            }

            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.animation_target = 0.0;
            }
        }

        Message::TrayNavigateLeft => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::SingleApp { app_id, navigation } = &tray_state.current_view {
                    if navigation.can_go_left && navigation.current_app_index > 0 {
                        let new_app_id = &navigation.app_order[navigation.current_app_index - 1];
                        let new_navigation = tray_state.tree.get_app_navigation(new_app_id);
                        tray_state.current_view = TrayViewState::SingleApp {
                            app_id: new_app_id.clone(),
                            navigation: new_navigation,
                        };
                        tray_state.selected_index = Some(0);
                    }
                }
            }
        }

        Message::TrayNavigateRight => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::SingleApp { app_id, navigation } = &tray_state.current_view {
                    if navigation.can_go_right && navigation.current_app_index < navigation.app_order.len() - 1 {
                        let new_app_id = &navigation.app_order[navigation.current_app_index + 1];
                        let new_navigation = tray_state.tree.get_app_navigation(new_app_id);
                        tray_state.current_view = TrayViewState::SingleApp {
                            app_id: new_app_id.clone(),
                            navigation: new_navigation,
                        };
                        tray_state.selected_index = Some(0);
                    }
                }
            }
        }

        Message::TrayShowAggregated => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                let filter = if tray_state.filter_text.is_empty() { None } else { Some(tray_state.filter_text.clone()) };
                let items = tray_state.tree.get_aggregated_menu(filter.as_ref().map(String::as_str));
                tray_state.current_view = TrayViewState::Aggregated { items, filter };
                tray_state.selected_index = Some(0);
            }
        }

        Message::TrayShowFavorites => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                let items = tray_state.tree.get_favorites_menu();
                tray_state.current_view = TrayViewState::Favorites { items };
                tray_state.selected_index = Some(0);
            }
        }

        Message::TrayToggleFavorite(app_id, item_id) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                let toggled = tray_state.tree.toggle_favorite(&item_id);
                info!("Toggled favorite for {}: {}", item_id, toggled);
            }
        }

        Message::TrayFilterUpdate(filter) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.filter_text = filter.clone();
                if let TrayViewState::Aggregated { items, filter: current_filter } = &mut tray_state.current_view {
                    let new_filter = if filter.is_empty() { None } else { Some(filter) };
                    *items = tray_state.tree.get_aggregated_menu(new_filter.as_ref().map(String::as_str));
                    *current_filter = new_filter;
                }
            }
        }

        Message::TrayNetworkSnapshot(app_id, result) => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                if let TrayViewState::Network { app_id: current_app_id, data, loading, error } = &mut tray_state.current_view {
                    if current_app_id == &app_id {
                        *loading = false;
                        match result {
                            Ok(snapshot) => {
                                *data = Some(snapshot);
                                *error = None;
                            }
                            Err(err) => {
                                *data = None;
                                *error = Some(err);
                            }
                        }
                    }
                }
            }
        }

        // Legacy tray events (for compatibility)
        Message::TrayEvent(event) => {
            // Convert to enhanced tray event
            match event {
                tray::TrayEvent::Icons(icons) => {
                    // Convert legacy icons to enhanced icons
                    let enhanced_icons = icons.into_iter().map(|icon| {
                        enhanced_tray::TrayIcon {
                            key: icon.key,
                            service: icon.service,
                            path: icon.path,
                            id: icon.id,
                            title: icon.title,
                            icon_name: icon.icon_name,
                            status: icon.status,
                            has_menu: icon.has_menu,
                            menu_object_path: None, // Legacy icons don't have menu paths
                        }
                    }).collect();
                    
                    return Task::done(Message::EnhancedTrayEvent(enhanced_tray::TrayEvent::Icons(enhanced_icons)));
                }
            }
        }

        Message::TrayAnimateTick => {
            if let Some(tray_state) = bar.enhanced_tray.as_mut() {
                tray_state.animation_progress = animate_progress(
                    tray_state.animation_progress,
                    tray_state.animation_target,
                    0.12
                );
                if tray_state.animation_progress == 0.0 && tray_state.animation_target == 0.0 {
                    bar.enhanced_tray = None;
                }
            }
        }

        // Handle remaining network and spawn command messages
        Message::TrayNetworkRefresh(_) | Message::TrayNetworkToggle(_) | 
        Message::TrayNetworkToggleDone(_, _) | Message::TraySpawnCommand(_, _) | 
        Message::TraySpawnCommandDone(_, _) => {
            // These can be implemented as needed for network functionality
        }
    }
    Task::none()
}

// Helper functions for enhanced tray

fn get_current_menu_item_count(view_state: &TrayViewState) -> usize {
    match view_state {
        TrayViewState::SingleApp { .. } => {
            // This would need access to the tree to get the actual count
            // For now, return a default value
            0
        }
        TrayViewState::Aggregated { items, .. } |
        TrayViewState::Favorites { items } => items.len(),
        TrayViewState::Network { data, .. } => {
            if let Some(_) = data {
                3 // Typical network menu items
            } else {
                0
            }
        }
    }
}

fn get_menu_action_at_index(view_state: &TrayViewState, index: usize) -> Option<(String, enhanced_tray::TrayMenuAction)> {
    match view_state {
        TrayViewState::SingleApp { app_id, .. } => {
            // This would need access to the tree to get the actual action
            Some((app_id.clone(), enhanced_tray::TrayMenuAction::Activate))
        }
        TrayViewState::Aggregated { items, .. } |
        TrayViewState::Favorites { items } => {
            items.get(index).map(|item| (item.app_id.clone(), item.action.clone()))
        }
        TrayViewState::Network { app_id, .. } => {
            Some((app_id.clone(), enhanced_tray::TrayMenuAction::Activate))
        }
    }
}

fn animate_progress(current: f32, target: f32, rate: f32) -> f32 {
    if (current - target).abs() < 0.001 {
        target
    } else {
        current + (target - current) * rate
    }
}