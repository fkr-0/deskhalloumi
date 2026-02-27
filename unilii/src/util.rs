//! Miscellaneous helper utilities for the unilii project.

/// Compute a `_NET_WM_STRUT_PARTIAL` property suitable for a dock window on
/// the left or right side of the screen.  See the Extended Window
/// Manager Hints (EWMH) specification for details on how struts
/// reserve screen real estate for dock windows.  For a left‑side dock
/// the first four values are `(width, 0, 0, 0)` and for a right‑side
/// dock they are `(0, width, 0, 0)`.  The next eight values specify
/// the start and end coordinates of the reserved area along the
/// corresponding axis.  In both cases we reserve the entire height of
/// the screen.
pub fn build_strut_partial(side: DockSide, width: u32, screen_height: u32) -> [u32; 12] {
    match side {
        DockSide::Left => [
            width, // left
            0,     // right
            0,     // top
            0,     // bottom
            0,
            screen_height,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
        DockSide::Right => [
            0,
            width,
            0,
            0,
            0,
            0,
            0,
            screen_height,
            0,
            0,
            0,
            0,
        ],
    }
}

/// Enumeration of the two sides of the screen on which the bar can
/// reserve space.  Currently we only support left or right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockSide {
    Left,
    Right,
}