use crate::enhanced_tray::EnhancedTrayState;
use crate::tray;

pub fn apply_animation_tick(enhanced_tray_state: &mut Option<EnhancedTrayState>) {
    let Some(tray_state) = enhanced_tray_state.as_mut() else {
        return;
    };

    tray_state.animation_progress = tray::animate_progress(
        tray_state.animation_progress,
        tray_state.animation_target,
        0.12,
    );

    if tray_state.animation_progress == 0.0 && tray_state.animation_target == 0.0 {
        *enhanced_tray_state = None;
    }
}

#[cfg(test)]
mod tests {
    use super::apply_animation_tick;
    use crate::enhanced_tray::EnhancedTrayState;

    #[test]
    fn animation_tick_moves_progress_toward_target() {
        let mut state = Some(EnhancedTrayState::new());
        let tray_state = state.as_mut().expect("state exists");
        tray_state.animation_progress = 0.0;
        tray_state.animation_target = 1.0;

        apply_animation_tick(&mut state);

        let tray_state = state.expect("state remains while opening");
        assert!(tray_state.animation_progress > 0.0);
        assert!(tray_state.animation_progress < 1.0);
    }

    #[test]
    fn animation_tick_drops_state_after_close_reaches_zero() {
        let mut state = Some(EnhancedTrayState::new());
        let tray_state = state.as_mut().expect("state exists");
        tray_state.animation_progress = 0.0;
        tray_state.animation_target = 0.0;

        apply_animation_tick(&mut state);

        assert!(state.is_none());
    }
}
