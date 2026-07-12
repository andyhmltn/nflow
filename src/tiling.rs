use crate::types::{LayoutTree, Rect, WindowId};

pub fn compute_layout(screen: Rect, layout: &LayoutTree) -> Vec<(WindowId, Rect)> {
    compute_layout_with_gaps(screen, layout, 0.0, 0.0)
}

pub fn compute_layout_with_gaps(
    screen: Rect,
    layout: &LayoutTree,
    outer_gap: f64,
    inner_gap: f64,
) -> Vec<(WindowId, Rect)> {
    let non_empty_cols: Vec<_> = layout
        .columns
        .iter()
        .filter(|c| !c.windows.is_empty())
        .collect();
    if non_empty_cols.is_empty() {
        return vec![];
    }

    let usable_x = screen.x + outer_gap;
    let usable_y = screen.y + outer_gap;
    let usable_width = screen.width - outer_gap * 2.0;
    let usable_height = screen.height - outer_gap * 2.0;

    let col_count = non_empty_cols.len();
    let total_horizontal_gap = inner_gap * col_count.saturating_sub(1) as f64;
    let weight_sum: f64 = non_empty_cols.iter().map(|c| c.weight.max(0.0)).sum();
    let weight_sum = if weight_sum <= 0.0 {
        col_count as f64
    } else {
        weight_sum
    };
    let usable_for_cols = usable_width - total_horizontal_gap;

    let mut col_x = usable_x;
    non_empty_cols
        .iter()
        .flat_map(|col| {
            let weight = if col.weight > 0.0 { col.weight } else { 1.0 };
            let col_width = usable_for_cols * (weight / weight_sum);
            let win_count = col.windows.len();
            let total_vertical_gap = inner_gap * win_count.saturating_sub(1) as f64;
            let win_height = (usable_height - total_vertical_gap) / win_count as f64;
            let this_x = col_x;
            col_x += col_width + inner_gap;

            col.windows
                .iter()
                .enumerate()
                .map(move |(win_idx, &window_id)| {
                    let frame = Rect {
                        x: this_x,
                        y: usable_y + win_idx as f64 * (win_height + inner_gap),
                        width: col_width,
                        height: win_height,
                    };
                    (window_id, frame)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Column, LayoutTree};

    fn screen_1920_1080() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        }
    }

    fn layout_with_cols(cols: Vec<Vec<WindowId>>) -> LayoutTree {
        let columns = cols
            .into_iter()
            .map(|windows| Column {
                windows,
                weight: 1.0,
            })
            .collect();
        LayoutTree { columns }
    }

    #[test]
    fn empty_layout_returns_empty_vec() {
        let layout = LayoutTree::new();
        let result = compute_layout(screen_1920_1080(), &layout);
        assert!(result.is_empty());
    }

    #[test]
    fn empty_columns_are_skipped() {
        let layout = layout_with_cols(vec![vec![], vec![]]);
        let result = compute_layout(screen_1920_1080(), &layout);
        assert!(result.is_empty());
    }

    #[test]
    fn single_window_fills_screen() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1]]);
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 1);
        assert_eq!(result[0].1, screen);
    }

    #[test]
    fn two_columns_split_width_equally() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1], vec![2]]);
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 2);

        let (id1, rect1) = result[0];
        let (id2, rect2) = result[1];

        assert_eq!(id1, 1);
        assert_eq!(id2, 2);

        assert_eq!(rect1.x, 0.0);
        assert_eq!(rect1.y, 0.0);
        assert_eq!(rect1.width, 960.0);
        assert_eq!(rect1.height, 1080.0);

        assert_eq!(rect2.x, 960.0);
        assert_eq!(rect2.y, 0.0);
        assert_eq!(rect2.width, 960.0);
        assert_eq!(rect2.height, 1080.0);
    }

    #[test]
    fn three_columns_split_width_equally() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1], vec![2], vec![3]]);
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 3);

        let col_width = 1920.0 / 3.0;
        for (i, (_, rect)) in result.iter().enumerate() {
            let expected_x = i as f64 * col_width;
            assert!((rect.x - expected_x).abs() < 1e-10);
            assert_eq!(rect.y, 0.0);
            assert!((rect.width - col_width).abs() < 1e-10);
            assert_eq!(rect.height, 1080.0);
        }
    }

    #[test]
    fn two_windows_in_one_column_split_height_equally() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1, 2]]);
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 2);

        let (id1, rect1) = result[0];
        let (id2, rect2) = result[1];

        assert_eq!(id1, 1);
        assert_eq!(rect1.x, 0.0);
        assert_eq!(rect1.y, 0.0);
        assert_eq!(rect1.width, 1920.0);
        assert_eq!(rect1.height, 540.0);

        assert_eq!(id2, 2);
        assert_eq!(rect2.x, 0.0);
        assert_eq!(rect2.y, 540.0);
        assert_eq!(rect2.width, 1920.0);
        assert_eq!(rect2.height, 540.0);
    }

    #[test]
    fn two_by_two_grid() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1, 2], vec![3, 4]]);
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 4);

        let expected = [
            (
                1u32,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    width: 960.0,
                    height: 540.0,
                },
            ),
            (
                2u32,
                Rect {
                    x: 0.0,
                    y: 540.0,
                    width: 960.0,
                    height: 540.0,
                },
            ),
            (
                3u32,
                Rect {
                    x: 960.0,
                    y: 0.0,
                    width: 960.0,
                    height: 540.0,
                },
            ),
            (
                4u32,
                Rect {
                    x: 960.0,
                    y: 540.0,
                    width: 960.0,
                    height: 540.0,
                },
            ),
        ];

        for (actual, exp) in result.iter().zip(expected.iter()) {
            assert_eq!(actual.0, exp.0);
            assert!((actual.1.x - exp.1.x).abs() < 1e-10);
            assert!((actual.1.y - exp.1.y).abs() < 1e-10);
            assert!((actual.1.width - exp.1.width).abs() < 1e-10);
            assert!((actual.1.height - exp.1.height).abs() < 1e-10);
        }
    }

    #[test]
    fn three_windows_in_one_column() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![10, 20, 30]]);
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 3);

        let win_height = 1080.0 / 3.0;
        for (i, (id, rect)) in result.iter().enumerate() {
            let expected_id = (i as u32 + 1) * 10;
            assert_eq!(*id, expected_id);
            assert_eq!(rect.x, 0.0);
            assert!((rect.y - i as f64 * win_height).abs() < 1e-10);
            assert_eq!(rect.width, 1920.0);
            assert!((rect.height - win_height).abs() < 1e-10);
        }
    }

    #[test]
    fn empty_col_among_non_empty_cols_is_skipped() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1], vec![], vec![2]]);
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 2);

        assert_eq!(result[0].0, 1);
        assert_eq!(result[0].1.width, 960.0);
        assert_eq!(result[1].0, 2);
        assert_eq!(result[1].1.x, 960.0);
    }

    #[test]
    fn ultrawide_screen_dimensions() {
        let screen = Rect {
            x: 0.0,
            y: 0.0,
            width: 5120.0,
            height: 1440.0,
        };
        let layout = layout_with_cols(vec![vec![1], vec![2], vec![3], vec![4]]);
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 4);

        let col_width = 5120.0 / 4.0;
        for (i, (_, rect)) in result.iter().enumerate() {
            assert!((rect.x - i as f64 * col_width).abs() < 1e-10);
            assert_eq!(rect.y, 0.0);
            assert!((rect.width - col_width).abs() < 1e-10);
            assert_eq!(rect.height, 1440.0);
        }
    }

    #[test]
    fn non_zero_screen_origin() {
        let screen = Rect {
            x: 100.0,
            y: 50.0,
            width: 1920.0,
            height: 1080.0,
        };
        let layout = layout_with_cols(vec![vec![1], vec![2]]);
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 2);

        assert_eq!(result[0].1.x, 100.0);
        assert_eq!(result[0].1.y, 50.0);
        assert_eq!(result[1].1.x, 1060.0);
        assert_eq!(result[1].1.y, 50.0);
    }

    #[test]
    fn no_gaps_and_no_overlaps_two_columns_two_windows_each() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1, 2], vec![3, 4]]);
        let result = compute_layout(screen, &layout);

        let total_area: f64 = result.iter().map(|(_, r)| r.width * r.height).sum();
        let screen_area = screen.width * screen.height;
        assert!(
            (total_area - screen_area).abs() < 1e-6,
            "total area {total_area} != screen area {screen_area}"
        );

        for i in 0..result.len() {
            for j in (i + 1)..result.len() {
                let a = result[i].1;
                let b = result[j].1;
                let overlap = rects_overlap(a, b);
                assert!(!overlap, "rects {:?} and {:?} overlap", a, b);
            }
        }
    }

    #[test]
    fn no_gaps_single_column_three_windows() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1, 2, 3]]);
        let result = compute_layout(screen, &layout);

        let total_area: f64 = result.iter().map(|(_, r)| r.width * r.height).sum();
        let screen_area = screen.width * screen.height;
        assert!((total_area - screen_area).abs() < 1e-6);
    }

    fn rects_overlap(a: Rect, b: Rect) -> bool {
        let ax2 = a.x + a.width;
        let ay2 = a.y + a.height;
        let bx2 = b.x + b.width;
        let by2 = b.y + b.height;

        a.x < bx2 && ax2 > b.x && a.y < by2 && ay2 > b.y
    }

    #[test]
    fn outer_gap_shrinks_window_to_padded_region() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1]]);
        let result = compute_layout_with_gaps(screen, &layout, 10.0, 0.0);
        let frame = result[0].1;
        assert_eq!(frame.x, 10.0);
        assert_eq!(frame.y, 10.0);
        assert_eq!(frame.width, 1900.0);
        assert_eq!(frame.height, 1060.0);
    }

    #[test]
    fn inner_gap_separates_two_columns() {
        let screen = screen_1920_1080();
        let layout = layout_with_cols(vec![vec![1], vec![2]]);
        let result = compute_layout_with_gaps(screen, &layout, 0.0, 8.0);
        let (left, right) = (result[0].1, result[1].1);
        assert!((left.x + left.width + 8.0 - right.x).abs() < 1e-9);
        assert!((left.width + right.width + 8.0 - 1920.0).abs() < 1e-9);
    }

    #[test]
    fn columns_with_weights_split_proportionally() {
        let screen = Rect {
            x: 0.0,
            y: 0.0,
            width: 4000.0,
            height: 1000.0,
        };
        let layout = LayoutTree {
            columns: vec![
                Column {
                    windows: vec![1],
                    weight: 1.0,
                },
                Column {
                    windows: vec![2],
                    weight: 2.0,
                },
                Column {
                    windows: vec![3],
                    weight: 1.0,
                },
            ],
        };
        let result = compute_layout(screen, &layout);
        assert_eq!(result.len(), 3);

        assert!((result[0].1.width - 1000.0).abs() < 1e-9);
        assert!((result[1].1.width - 2000.0).abs() < 1e-9);
        assert!((result[2].1.width - 1000.0).abs() < 1e-9);

        assert!((result[0].1.x - 0.0).abs() < 1e-9);
        assert!((result[1].1.x - 1000.0).abs() < 1e-9);
        assert!((result[2].1.x - 3000.0).abs() < 1e-9);
    }

    #[test]
    fn weights_respect_inner_gaps() {
        let screen = Rect {
            x: 0.0,
            y: 0.0,
            width: 4020.0,
            height: 1000.0,
        };
        let layout = LayoutTree {
            columns: vec![
                Column {
                    windows: vec![1],
                    weight: 1.0,
                },
                Column {
                    windows: vec![2],
                    weight: 2.0,
                },
                Column {
                    windows: vec![3],
                    weight: 1.0,
                },
            ],
        };
        let result = compute_layout_with_gaps(screen, &layout, 0.0, 10.0);

        assert!((result[0].1.width - 1000.0).abs() < 1e-9);
        assert!((result[1].1.width - 2000.0).abs() < 1e-9);
        assert!((result[2].1.width - 1000.0).abs() < 1e-9);

        assert!((result[1].1.x - (result[0].1.x + result[0].1.width + 10.0)).abs() < 1e-9);
        assert!((result[2].1.x - (result[1].1.x + result[1].1.width + 10.0)).abs() < 1e-9);
    }
}
