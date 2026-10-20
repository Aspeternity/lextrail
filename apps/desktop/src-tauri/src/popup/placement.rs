//! Where to put the popup relative to the cursor. All values are physical pixels.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    fn right(self) -> i32 {
        self.x + self.width
    }

    fn bottom(self) -> i32 {
        self.y + self.height
    }
}

/// Top-left corner for a popup of `size` near `cursor`, inside `work_area`.
///
/// Prefers below-right of the cursor (keeps the selected line visible), flips to the
/// left / above when there is no room, and finally clamps to the work area. If the
/// popup is larger than the work area, its top-left corner stays visible.
pub fn place(cursor: Point, size: Size, work_area: Rect, gap: i32) -> Point {
    Point {
        x: axis(cursor.x, size.width, work_area.x, work_area.right(), gap),
        y: axis(cursor.y, size.height, work_area.y, work_area.bottom(), gap),
    }
}

/// The popup's height limits, in logical pixels (D046 supplement).
pub const MIN_HEIGHT: f64 = 72.0;
pub const MAX_HEIGHT: f64 = 520.0;
/// The most of its monitor's work area height the popup may take.
pub const MAX_AREA_SHARE: f64 = 0.6;

/// The popup's height in physical pixels for content `natural` logical pixels tall, on a
/// monitor with `scale` and a work area `area_height` physical pixels tall: the content's
/// height, at most `max` ([`MAX_HEIGHT`]) or [`MAX_AREA_SHARE`] of the work area, whichever is
/// smaller (beyond it, the body scrolls), and at least [`MIN_HEIGHT`] (which wins over
/// the 60% on a tiny work area). Never more than the work area itself.
pub fn popup_height(natural: f64, scale: f64, area_height: i32, max: f64) -> i32 {
    let min = (MIN_HEIGHT * scale).round() as i32;
    let max = ((max * scale).round() as i32)
        .min((f64::from(area_height) * MAX_AREA_SHARE).floor() as i32);
    let wanted = (natural.max(0.0) * scale).ceil() as i32;
    wanted.min(max).max(min).min(area_height.max(1))
}

/// Where a shown popup at `at` goes when its height changes to `size.height` (an entry
/// unfolded or folded): its top edge stays and it grows downwards; when the bottom would
/// leave the work area, it moves up as far as needed, but never above the work area.
pub fn resize_in_place(at: Point, size: Size, work_area: Rect) -> Point {
    Point {
        x: at.x,
        y: at.y.min(work_area.bottom() - size.height).max(work_area.y),
    }
}

fn axis(cursor: i32, len: i32, start: i32, end: i32, gap: i32) -> i32 {
    let after = cursor + gap;
    let before = cursor - gap - len;
    let preferred = if after + len <= end {
        after
    } else if before >= start {
        before
    } else {
        // Neither side fits: push it as far as needed to stay on screen.
        end - len
    };
    preferred.min(end - len).max(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1040, // 1080 minus the taskbar
    };
    const SIZE: Size = Size {
        width: 380,
        height: 260,
    };
    const GAP: i32 = 12;

    fn at(x: i32, y: i32) -> Point {
        Point { x, y }
    }

    #[test]
    fn below_right_of_cursor_when_there_is_room() {
        assert_eq!(place(at(500, 400), SIZE, AREA, GAP), at(512, 412));
    }

    #[test]
    fn flips_left_near_right_edge() {
        assert_eq!(
            place(at(1800, 400), SIZE, AREA, GAP),
            at(1800 - 12 - 380, 412)
        );
    }

    #[test]
    fn flips_above_near_bottom_edge() {
        assert_eq!(
            place(at(500, 1000), SIZE, AREA, GAP),
            at(512, 1000 - 12 - 260)
        );
    }

    #[test]
    fn flips_both_in_bottom_right_corner() {
        assert_eq!(
            place(at(1910, 1030), SIZE, AREA, GAP),
            at(1910 - 12 - 380, 1030 - 12 - 260)
        );
    }

    #[test]
    fn exactly_fitting_on_the_right_does_not_flip() {
        let x = 1920 - 380 - 12;
        assert_eq!(place(at(x, 400), SIZE, AREA, GAP).x, x + 12);
    }

    #[test]
    fn small_area_where_neither_side_fits_stays_inside() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 500,
            height: 400,
        };
        let p = place(at(250, 200), SIZE, area, GAP);
        assert!(p.x >= 0 && p.x + SIZE.width <= area.width, "{p:?}");
        assert!(p.y >= 0 && p.y + SIZE.height <= area.height, "{p:?}");
    }

    #[test]
    fn popup_larger_than_area_keeps_top_left_visible() {
        let area = Rect {
            x: 100,
            y: 50,
            width: 200,
            height: 100,
        };
        assert_eq!(place(at(150, 80), SIZE, area, GAP), at(100, 50));
    }

    #[test]
    fn secondary_monitor_with_negative_origin() {
        // Monitor to the left of the primary one, taskbar at the top.
        let area = Rect {
            x: -2560,
            y: 40,
            width: 2560,
            height: 1400,
        };
        assert_eq!(place(at(-1000, 500), SIZE, area, GAP), at(-988, 512));
        // Near its top edge with the popup flipped up, it must stay below the taskbar.
        let near_top = place(
            at(-1000, 45),
            Size {
                width: 380,
                height: 1400,
            },
            area,
            GAP,
        );
        assert_eq!(near_top.y, 40);
    }

    #[test]
    fn height_follows_short_content_with_a_floor() {
        // 1.0x, 1040 px work area: a one-line hint is raised to the minimum.
        assert_eq!(popup_height(40.0, 1.0, 1040, MAX_HEIGHT), 72);
        assert_eq!(popup_height(150.4, 1.0, 1040, MAX_HEIGHT), 151);
        // 1.5x: the same content in physical pixels.
        assert_eq!(popup_height(150.4, 1.5, 1560, MAX_HEIGHT), 226);
        assert_eq!(popup_height(10.0, 1.5, 1560, MAX_HEIGHT), 108);
    }

    #[test]
    fn height_is_capped_at_520_or_60_percent_of_the_work_area() {
        // Tall content on a large work area: 520 logical.
        assert_eq!(popup_height(2000.0, 1.0, 1040, MAX_HEIGHT), 520);
        assert_eq!(popup_height(2000.0, 1.5, 1560, MAX_HEIGHT), 780);
        assert_eq!(popup_height(2000.0, 2.0, 2080, MAX_HEIGHT), 1040);
        // A short work area (1366x768 laptop, 728 px): 60% of it, 436.
        assert_eq!(popup_height(2000.0, 1.0, 728, MAX_HEIGHT), 436);
        // 1.25x on 1080p (work area 1032 physical): 60% = 619 < 650.
        assert_eq!(popup_height(2000.0, 1.25, 1032, MAX_HEIGHT), 619);
        // Exactly at the cap is not scrolled.
        assert_eq!(popup_height(520.0, 1.0, 1040, MAX_HEIGHT), 520);
    }

    #[test]
    fn height_never_exceeds_a_tiny_work_area() {
        // 60% (60) is below the minimum: the minimum wins, still inside the work area.
        assert_eq!(popup_height(2000.0, 1.0, 100, MAX_HEIGHT), 72);
        assert_eq!(popup_height(10.0, 2.0, 100, MAX_HEIGHT), 100);
    }

    #[test]
    fn taller_popup_near_the_bottom_grows_upwards() {
        // The same cursor near the bottom: the taller the popup, the higher its top.
        let cursor = at(500, 900);
        let short = place(
            cursor,
            Size {
                width: 380,
                height: 120,
            },
            AREA,
            GAP,
        );
        let tall = place(
            cursor,
            Size {
                width: 380,
                height: 520,
            },
            AREA,
            GAP,
        );
        assert_eq!(short, at(512, 912));
        assert_eq!(tall, at(512, 900 - 12 - 520));
        assert!(tall.y + 520 <= AREA.height);
    }

    #[test]
    fn unfolding_keeps_the_top_and_grows_down() {
        let size = Size {
            width: 380,
            height: 400,
        };
        assert_eq!(resize_in_place(at(512, 300), size, AREA), at(512, 300));
    }

    #[test]
    fn unfolding_without_room_below_moves_up_but_stays_inside() {
        let size = Size {
            width: 380,
            height: 400,
        };
        // Bottom would be 1100 > 1040: moved up by 60.
        assert_eq!(resize_in_place(at(512, 700), size, AREA), at(512, 640));
        // Taller than the space above too: flush with the work area's top.
        let huge = Size {
            width: 380,
            height: 1200,
        };
        assert_eq!(resize_in_place(at(512, 700), huge, AREA), at(512, 0));
        // Folding (shorter) never moves it.
        let short = Size {
            width: 380,
            height: 100,
        };
        assert_eq!(resize_in_place(at(512, 700), short, AREA), at(512, 700));
    }

    #[test]
    fn cursor_outside_work_area_is_clamped() {
        // Cursor over the taskbar: flipping above the cursor would still overhang the
        // work area, so the popup ends flush with its bottom.
        let p = place(at(900, 1070), SIZE, AREA, GAP);
        assert_eq!(p, at(912, 1040 - 260));
    }
}
