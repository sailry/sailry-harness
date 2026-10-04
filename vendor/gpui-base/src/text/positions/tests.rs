use super::*;
use gpui::{FontId, GlyphId, LineLayout, ShapedGlyph, ShapedRun, WrapBoundary, px};
use std::sync::Arc;

fn layout(indices: &[usize], wraps: &[usize]) -> WrappedLineLayout {
    WrappedLineLayout {
        unwrapped_layout: Arc::new(LineLayout {
            width: px(300.),
            len: 24,
            runs: vec![
                ShapedRun {
                    font_id: FontId(0),
                    glyphs: Vec::new(),
                },
                ShapedRun {
                    font_id: FontId(1),
                    glyphs: indices
                        .iter()
                        .enumerate()
                        .map(|(offset, &index)| ShapedGlyph {
                            id: GlyphId(offset as u32),
                            index,
                            position: point(px(offset as f32 * 10.319), px(0.)),
                            is_emoji: offset % 2 == 0,
                        })
                        .collect(),
                },
            ],
            ..Default::default()
        }),
        wrap_boundaries: wraps
            .iter()
            .map(|&glyph_ix| WrapBoundary {
                run_ix: 1,
                glyph_ix,
            })
            .collect(),
        wrap_width: Some(px(70.)),
    }
}

#[test]
fn matches_shaped_glyph_and_wrap_boundaries() {
    for indices in [
        vec![0, 1, 2, 3, 4, 5, 6, 12, 23],
        vec![0, 0, 3, 7, 7, 8, 12, 16, 23],
        vec![7, 3, 0, 12, 8, 16, 23, 20, 19],
    ] {
        for wraps in [vec![], vec![3], vec![0, 3, 6], vec![3, 3, 8], vec![6, 3, 8]] {
            let original = layout(&indices, &wraps);
            let origin = point(px(25.317), px(-73.728));
            let indexed = Line::new(&original, 5, origin, px(21.));
            assert_eq!(indexed.position_for_index(4), None);
            for index in 0..=original.len() + 1 {
                assert_eq!(
                    indexed.position_for_index(index + 5),
                    original
                        .position_for_index(index, px(21.))
                        .map(|pos| pos + origin),
                    "indices={indices:?}, wraps={wraps:?}, index={index}",
                );
            }
        }
    }
}

#[test]
fn preserves_newlines_and_empty_lines() {
    let mut empty = layout(&[], &[]);
    Arc::get_mut(&mut empty.unwrapped_layout).unwrap().len = 0;
    let filled = layout(&[0, 3, 7, 12, 23], &[2]);
    let origins = [
        point(px(8.), px(10.)),
        point(px(8.), px(30.)),
        point(px(8.), px(70.)),
    ];
    let positions = Positions {
        lines: vec![
            Line::new(&empty, 0, origins[0], px(20.)),
            Line::new(&filled, 1, origins[1], px(20.)),
            Line::new(&empty, 26, origins[2], px(20.)),
        ],
    };
    assert_eq!(positions.position_for_index(0), Some(origins[0]));
    for index in 0..=filled.len() {
        assert_eq!(
            positions.position_for_index(index + 1),
            filled
                .position_for_index(index, px(20.))
                .map(|pos| pos + origins[1])
        );
    }
    assert_eq!(positions.position_for_index(26), Some(origins[2]));
    assert_eq!(positions.position_for_index(27), None);
}
