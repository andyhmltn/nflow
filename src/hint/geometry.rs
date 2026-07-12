use crate::types::Rect;

pub fn flip_y(ax_y: f64, height: f64, screen_height: f64) -> f64 {
    screen_height - (ax_y + height)
}

pub fn center(rect: Rect) -> (f64, f64) {
    (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

pub fn is_usable(rect: Rect, screen: Rect) -> bool {
    if rect.width <= 1.0 || rect.height <= 1.0 {
        return false;
    }
    let (cx, cy) = center(rect);
    cx >= screen.x
        && cx <= screen.x + screen.width
        && cy >= screen.y
        && cy <= screen.y + screen.height
}

pub fn dedup(rects: Vec<Rect>) -> Vec<Rect> {
    let mut seen: Vec<(i64, i64)> = Vec::new();
    let mut out = Vec::new();
    for rect in rects {
        let (cx, cy) = center(rect);
        let key = (cx.round() as i64, cy.round() as i64);
        if !seen.contains(&key) {
            seen.push(key);
            out.push(rect);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect {
            x,
            y,
            width: w,
            height: h,
        }
    }

    #[test]
    fn flip_maps_top_left_to_bottom_left() {
        assert_eq!(flip_y(0.0, 20.0, 1000.0), 980.0);
        assert_eq!(flip_y(100.0, 20.0, 1000.0), 880.0);
    }

    #[test]
    fn center_is_global_top_left_coords() {
        let (cx, cy) = center(r(10.0, 20.0, 100.0, 40.0));
        assert_eq!((cx, cy), (60.0, 40.0));
    }

    #[test]
    fn rejects_degenerate_and_offscreen() {
        let screen = r(0.0, 0.0, 1000.0, 800.0);
        assert!(!is_usable(r(10.0, 10.0, 0.0, 30.0), screen));
        assert!(!is_usable(r(-500.0, 10.0, 100.0, 30.0), screen));
        assert!(is_usable(r(10.0, 10.0, 100.0, 30.0), screen));
    }

    #[test]
    fn dedup_collapses_shared_centers() {
        let rects = vec![
            r(0.0, 0.0, 100.0, 40.0),
            r(0.0, 0.0, 100.0, 40.0),
            r(200.0, 0.0, 100.0, 40.0),
        ];
        assert_eq!(dedup(rects).len(), 2);
    }
}
