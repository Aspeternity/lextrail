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
    fn cursor_outside_work_area_is_clamped() {
        // Cursor over the taskbar: flipping above the cursor would still overhang the
        // work area, so the popup ends flush with its bottom.
        let p = place(at(900, 1070), SIZE, AREA, GAP);
        assert_eq!(p, at(912, 1040 - 260));
    }
}
