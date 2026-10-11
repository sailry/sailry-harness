// Modified by Longbridge for gpui-fast.
//! Tests of scroll layers over virtual lists (M6): a `uniform_list` or
//! `list` whose layer is extended by the rows a scroll uncovers, rendering
//! only those.

use crate::fast::layers::policy::{Decision, last_decision};
use crate::{
    AnyWindowHandle, App, AppContext as _, Bounds, GlobalElementId, Hsla, ScaledPixels, Scene,
    ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, Window, point, px,
};

/// How tall the list is, in pixels.
const VIEWPORT_HEIGHT: f32 = 100.;
/// How wide the list is, in pixels.
const VIEWPORT_WIDTH: f32 = 200.;
/// How tall a row of a uniform list is, in pixels.
const ROW_HEIGHT: f32 = 20.;

fn with_window<R>(
    cx: &mut TestAppContext,
    window: AnyWindowHandle,
    f: impl FnOnce(&mut Window, &mut App) -> R,
) -> R {
    cx.update_window(window, |_, window, cx| f(window, cx))
        .unwrap()
}

fn draw(cx: &mut TestAppContext, window: AnyWindowHandle) {
    with_window(cx, window, |window, cx| window.draw(cx).clear(cx));
}

/// Scrolls by `dy` with the wheel over the list, and draws the frame that
/// follows, unless the scroll drew it.
fn wheel(cx: &mut TestAppContext, window: AnyWindowHandle, dy: f32) {
    let frame = with_window(cx, window, |window, _| window.fast_layers.frame);
    with_window(cx, window, |window, cx| {
        window.dispatch_event(
            crate::PlatformInput::ScrollWheel(ScrollWheelEvent {
                position: point(px(20.), px(20.)),
                delta: ScrollDelta::Pixels(point(px(0.), px(dy))),
                modifiers: Default::default(),
                touch_phase: TouchPhase::Moved,
            }),
            cx,
        );
    });
    if with_window(cx, window, |window, _| window.fast_layers.frame) == frame {
        draw(cx, window);
    }
}

/// The id of the window's only layer.
fn layer_id(cx: &mut TestAppContext, window: AnyWindowHandle) -> GlobalElementId {
    with_window(cx, window, |window, _| {
        assert_eq!(window.fast_layers.layers.len(), 1, "one layer");
        window.fast_layers.layers.keys().next().unwrap().clone()
    })
}

/// What the window's only scroll container decided in the last frame.
fn decision(cx: &mut TestAppContext, window: AnyWindowHandle) -> Option<Decision> {
    let id = layer_id(cx, window);
    with_window(cx, window, |window, _| last_decision(window, &id))
}

/// The rows the window's only layer holds.
fn held_rows(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<usize> {
    with_window(cx, window, |window, _| {
        let layer = window.fast_layers.layers.values().next().expect("a layer");
        layer.rows.held().collect()
    })
}

/// A row's colour.
fn row_color(row: usize) -> Hsla {
    crate::hsla((row % 97) as f32 / 97., 0.5, 0.5, 1.)
}

/// The quads `scene` draws, with the layers it composites expanded: each
/// layer's content moved to where its tiles are composited and clipped to
/// their viewport, as drawing the content straight into the frame would
/// have put it. Each quad is its bounds, the part of them its content mask
/// lets it draw, and its colour, sorted.
fn expanded_quads(scene: &Scene) -> Vec<String> {
    let mut quads = Vec::new();
    let describe = |bounds: Bounds<ScaledPixels>,
                    mask: Bounds<ScaledPixels>,
                    background: &crate::Background| {
        let drawn = bounds.intersect(&mask);
        format!("{bounds:?} {drawn:?} {background:?}")
    };
    for quad in &scene.quads {
        quads.push(describe(
            quad.bounds,
            quad.content_mask.bounds,
            &quad.background,
        ));
    }
    for frame in &scene.layers.frames {
        let sprite = scene
            .polychrome_sprites
            .iter()
            .find_map(|sprite| {
                crate::decode_layer_tile(sprite.tile.texture_id, sprite.tile.tile_id)
                    .filter(|(key, _)| *key == frame.key)
                    .map(|(_, coord)| (coord, sprite))
            })
            .expect("a composited layer has tiles");
        let (coord, sprite) = sprite;
        let tile = frame.tile_bounds(coord);
        let translation = sprite.bounds.origin - tile.origin;
        let viewport = sprite.content_mask.bounds;
        let mut content = Scene::default();
        for operation in frame.content.operations() {
            if let crate::scene::PaintOperation::Primitive(primitive) = operation {
                content.insert_primitive(primitive.clone());
            }
        }
        for quad in &content.quads {
            let bounds = Bounds {
                origin: quad.bounds.origin + translation,
                size: quad.bounds.size,
            };
            let mask = Bounds {
                origin: quad.content_mask.bounds.origin + translation,
                size: quad.content_mask.bounds.size,
            }
            .intersect(&viewport);
            if bounds.intersect(&mask).is_empty() {
                continue;
            }
            quads.push(describe(bounds, mask, &quad.background));
        }
    }
    quads.sort();
    quads
}

/// Whether the window's last frame drew a layer's tiles, its rows painted
/// afresh or kept.
fn composites(cx: &mut TestAppContext, window: AnyWindowHandle) -> bool {
    with_window(cx, window, |window, _| {
        !window.rendered_frame.scene.layers.frames.is_empty()
    })
}

/// How many frames on this thread composited a list's layer keeping the
/// rows it held, rendering at most those a scroll uncovered.
fn extended_frames() -> usize {
    crate::fast::layers::lists::extended_frames()
}

/// A pseudo-random sequence of wheel deltas, whole pixels in -60..=60.
fn wheel_deltas(count: usize) -> Vec<f32> {
    let mut state: u32 = 0x2545_f491;
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            ((state >> 16) % 121) as f32 - 60.
        })
        .collect()
}

/// A pseudo-random sequence of wheel deltas from `seed`, fractional and
/// whole, some of them half a device pixel at a scale of 1.25.
fn fractional_deltas(seed: u32, count: usize) -> Vec<f32> {
    const DELTAS: [f32; 6] = [-0.37, 0.37, -12.5, 12.5, -1.0, -7.3];
    let mut state: u32 = 0x2545_f491 ^ seed.wrapping_mul(0x9e37_79b9);
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            DELTAS[((state >> 16) as usize) % DELTAS.len()]
        })
        .collect()
}

/// Scrolls `with_layers` and `without_layers`, which draws without layers,
/// by each of `deltas`, checking after each that both draw the same quads;
/// returns in how many frames `with_layers` composited its layer.
fn compare_with_layers_off(
    cx: &mut TestAppContext,
    with_layers: AnyWindowHandle,
    without_layers: AnyWindowHandle,
    deltas: &[f32],
    label: &str,
) -> usize {
    with_window(cx, without_layers, |window, _| {
        window.set_scroll_layers(false)
    });
    draw(cx, without_layers);
    let mut composited = 0;
    for (frame, dy) in deltas.iter().copied().enumerate() {
        wheel(cx, with_layers, dy);
        wheel(cx, without_layers, dy);
        if composites(cx, with_layers) {
            composited += 1;
        }
        let expected = with_window(cx, without_layers, |window, _| {
            assert!(window.rendered_frame.scene.layers.frames.is_empty());
            expanded_quads(&window.rendered_frame.scene)
        });
        let actual = with_window(cx, with_layers, |window, _| {
            expanded_quads(&window.rendered_frame.scene)
        });
        assert_eq!(actual, expected, "{label}: frame {frame}, scrolled by {dy}");
    }
    composited
}

/// Sets the scale factor of `window`, not yet drawn, to `scale_factor`, and
/// draws it twice.
fn open_at(cx: &mut TestAppContext, window: AnyWindowHandle, scale_factor: f32) {
    if scale_factor != 1. {
        cx.test_window(window)
            .simulate_scale_factor_change(scale_factor);
    }
    draw(cx, window);
    draw(cx, window);
}

mod uniform {
    use super::{
        Decision, ROW_HEIGHT, VIEWPORT_HEIGHT, VIEWPORT_WIDTH, compare_with_layers_off, composites,
        decision, draw, expanded_quads, extended_frames, fractional_deltas, held_rows, open_at,
        row_color, wheel, wheel_deltas, with_window,
    };
    use crate::{
        AnyWindowHandle, Context, IntoElement, ParentElement as _, Render, Styled as _,
        TestAppContext, Window, WindowHandle, div, px, rgb,
    };
    use std::{cell::RefCell, ops::Range, rc::Rc};

    /// A white panel holding a uniform list of `count` rows of `row_height`
    /// px, 100 px tall, at the top left of the window. Every range of rows
    /// the list renders is logged.
    pub(super) struct UniformPage {
        pub(super) count: usize,
        pub(super) row_height: f32,
        tint: bool,
        background: crate::Hsla,
        pub(super) rendered: Rc<RefCell<Vec<Range<usize>>>>,
    }

    impl Render for UniformPage {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let rendered = self.rendered.clone();
            let row_height = self.row_height;
            let tint = self.tint;
            div().size_full().bg(self.background).child(
                crate::uniform_list(
                    "list",
                    self.count,
                    cx.processor(move |_, range: Range<usize>, _, _| {
                        rendered.borrow_mut().push(range.clone());
                        range
                            .map(|row| {
                                div().w(px(VIEWPORT_WIDTH)).h(px(row_height)).bg(if tint {
                                    rgb(0x224466).into()
                                } else {
                                    row_color(row)
                                })
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .w(px(VIEWPORT_WIDTH))
                .h(px(VIEWPORT_HEIGHT)),
            )
        }
    }

    pub(super) fn page(
        cx: &mut TestAppContext,
        count: usize,
    ) -> (WindowHandle<UniformPage>, Rc<RefCell<Vec<Range<usize>>>>) {
        page_with_rows_of(cx, count, ROW_HEIGHT, 1.)
    }

    fn page_with_rows_of(
        cx: &mut TestAppContext,
        count: usize,
        row_height: f32,
        scale_factor: f32,
    ) -> (WindowHandle<UniformPage>, Rc<RefCell<Vec<Range<usize>>>>) {
        let rendered = Rc::new(RefCell::new(Vec::new()));
        let log = rendered.clone();
        let window = cx.add_window(move |_, _| UniformPage {
            count,
            row_height,
            tint: false,
            background: rgb(0xffffff).into(),
            rendered: log,
        });
        open_at(cx, window.into(), scale_factor);
        (window, rendered)
    }

    /// The rows rendered since the log was last taken, leaving out the
    /// measured item (row 0 alone).
    fn rendered_rows(log: &Rc<RefCell<Vec<Range<usize>>>>) -> Vec<Range<usize>> {
        std::mem::take(&mut *log.borrow_mut())
            .into_iter()
            .filter(|range| *range != (0..1))
            .collect()
    }

    /// Scrolls until the list has a layer: promoted on the second scrolled
    /// frame.
    fn promote(cx: &mut TestAppContext, window: AnyWindowHandle) {
        wheel(cx, window, -ROW_HEIGHT);
        assert_eq!(decision(cx, window), Some(Decision::Bypass));
        wheel(cx, window, -ROW_HEIGHT);
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
    }

    #[crate::test]
    fn uniform_list_with_expensive_quarter_rate_updates_falls_back(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (handle, log) = page(cx, 1000);
        let window = handle.into();
        promote(cx, window);
        for frame in 0..64 {
            if frame % 4 == 0 {
                handle
                    .update(cx, |page, _, cx| {
                        page.tint = !page.tint;
                        cx.notify();
                    })
                    .unwrap();
            }
            rendered_rows(&log);
            wheel(cx, window, -ROW_HEIGHT);
        }
        assert_eq!(decision(cx, window), Some(Decision::Bypass));
        assert!(
            held_rows(cx, window).is_empty(),
            "demotion releases cached rows"
        );
        assert!(
            rendered_rows(&log)
                .iter()
                .map(|range| range.len())
                .sum::<usize>()
                <= 6,
            "the fallback renders the viewport, without layer overscan"
        );
        assert_eq!(
            with_window(cx, window, |window, _| window.layout_stats().layers_demoted),
            1
        );
        // Ongoing refreshes never re-promoted the layer. Once they stop,
        // the same container may cache again, with a fresh work budget.
        for _ in 0..64 {
            wheel(cx, window, -ROW_HEIGHT);
        }
        assert_eq!(decision(cx, window), Some(Decision::Composite));
        assert!(!held_rows(cx, window).is_empty());
    }

    #[crate::test]
    fn uniform_list_with_sparse_broad_updates_stays_demoted(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (handle, _) = page(cx, 1000);
        let window = handle.into();
        promote(cx, window);
        for frame in 0..128 {
            if frame % 16 == 0 {
                handle
                    .update(cx, |page, _, cx| {
                        page.tint = !page.tint;
                        cx.notify();
                    })
                    .unwrap();
            }
            wheel(cx, window, -ROW_HEIGHT);
        }
        assert_eq!(decision(cx, window), Some(Decision::Bypass));
        assert_eq!(
            with_window(cx, window, |window, _| window.layout_stats().layers_demoted),
            1
        );
    }

    #[crate::test]
    fn an_unbakeable_list_background_stops_rebuilding_overscan(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (handle, log) = page(cx, 1000);
        let window = handle.into();
        handle
            .update(cx, |page, _, cx| {
                page.background = crate::rgba(0xffffff80).into();
                cx.notify();
            })
            .unwrap();
        draw(cx, window);
        promote(cx, window);
        rendered_rows(&log);
        for _ in 0..20 {
            wheel(cx, window, -ROW_HEIGHT);
            assert_eq!(decision(cx, window), Some(Decision::Bypass));
            let rendered: usize = rendered_rows(&log).iter().map(|range| range.len()).sum();
            assert!(
                rendered <= (VIEWPORT_HEIGHT / ROW_HEIGHT) as usize + 2,
                "only visible rows should render while the background cannot be baked: {rendered}"
            );
        }
        handle
            .update(cx, |page, _, cx| {
                page.background = rgb(0xffffff).into();
                cx.notify();
            })
            .unwrap();
        for _ in 0..70 {
            wheel(cx, window, -ROW_HEIGHT);
        }
        assert!(
            composites(cx, window),
            "a bakeable background can recover after the retry delay"
        );
    }

    #[crate::test]
    fn uniform_list_renders_only_new_rows_when_scrolling(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (handle, log) = page(cx, 1000);
        let window = handle.into();
        promote(cx, window);
        // Rows 2..7 show, and ten rows of overscan below them and above them,
        // as far as row 0. Row 1 showed in the frame before, and was left out:
        // painted into the layer, it could have kept element states that a
        // list without a layer drops.
        let mut expected: Vec<usize> = [0].into_iter().chain(2..17).collect();
        assert_eq!(held_rows(cx, window), expected);
        rendered_rows(&log);

        wheel(cx, window, -ROW_HEIGHT);
        assert_eq!(decision(cx, window), Some(Decision::Composite));
        assert_eq!(
            rendered_rows(&log),
            vec![1..2, 17..18],
            "the row left out is rendered, and the row entering the overscan"
        );

        for step in 0..14 {
            wheel(cx, window, -ROW_HEIGHT);
            assert_eq!(decision(cx, window), Some(Decision::Composite));
            let new_row = 18 + step;
            assert_eq!(
                rendered_rows(&log),
                vec![new_row..new_row + 1],
                "step {step}: scrolling down one row renders the row entering the overscan"
            );
            // Rows that left the overscan are kept until they are more than
            // a quarter of the rows the layer is to hold, then dropped.
            let first = if step < 13 { 0 } else { new_row - 24 };
            expected = (first..new_row + 1).collect();
            assert_eq!(held_rows(cx, window), expected, "step {step}");
        }
        // Rows 17..22 show.
        assert_eq!(held_rows(cx, window), (7..32).collect::<Vec<_>>());

        wheel(cx, window, ROW_HEIGHT);
        assert_eq!(decision(cx, window), Some(Decision::Composite));
        assert_eq!(
            rendered_rows(&log),
            vec![6..7],
            "scrolling up one row renders the dropped row entering the overscan"
        );
        assert_eq!(held_rows(cx, window), (6..32).collect::<Vec<_>>());

        // Rows 16..21 show; a quarter of a row down, 16..22 do, all held.
        for _ in 0..2 {
            wheel(cx, window, -ROW_HEIGHT / 4.);
            assert_eq!(decision(cx, window), Some(Decision::Composite));
            assert_eq!(
                rendered_rows(&log),
                Vec::<Range<usize>>::new(),
                "a scroll that uncovers no row renders none"
            );
        }
    }

    /// A list whose content changes on every other frame while it scrolls, as
    /// a feed of 60 updates a second does at 120 Hz, would paint five
    /// viewports of rows on each of them: its layer is demoted by the fourth
    /// change within sixteen frames (the work guard may act earlier), and stays demoted while the feed goes on.
    #[crate::test]
    fn a_list_whose_content_keeps_changing_is_demoted(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (handle, _log) = page(cx, 1000);
        let window: AnyWindowHandle = handle.into();
        // Changes every row and draws the frame that follows, unless the
        // change drew it.
        let change = |cx: &mut TestAppContext| {
            let frame = with_window(cx, window, |window, _| window.fast_layers.frame);
            handle
                .update(cx, |page, _, cx| {
                    // A new row height: every row the layer holds changes.
                    page.row_height = if page.row_height == ROW_HEIGHT {
                        ROW_HEIGHT + 1.
                    } else {
                        ROW_HEIGHT
                    };
                    cx.notify();
                })
                .unwrap();
            if with_window(cx, window, |window, _| window.fast_layers.frame) == frame {
                draw(cx, window);
            }
        };
        let demoted = |cx: &mut TestAppContext| {
            with_window(cx, window, |window, _| window.layout_stats().layers_demoted)
        };
        promote(cx, window);
        for _ in 0..4 {
            change(cx);
            if decision(cx, window) == Some(Decision::Bypass) {
                break;
            }
            wheel(cx, window, -ROW_HEIGHT);
        }
        assert_eq!(decision(cx, window), Some(Decision::Bypass));
        assert_eq!(demoted(cx), 1);
        for step in 0..20 {
            if step % 2 == 0 {
                change(cx);
            } else {
                wheel(cx, window, -ROW_HEIGHT);
            }
            assert_eq!(decision(cx, window), Some(Decision::Bypass), "step {step}");
        }
    }

    #[crate::test]
    fn uniform_list_matches_layers_off(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (with_layers, _) = page(cx, 300);
        let (without_layers, _) = page(cx, 300);
        let with_layers: AnyWindowHandle = with_layers.into();
        let without_layers: AnyWindowHandle = without_layers.into();
        with_window(cx, without_layers, |window, _| {
            window.set_scroll_layers(false)
        });
        draw(cx, without_layers);

        let mut composited = 0;
        let extended = extended_frames();
        for (frame, dy) in wheel_deltas(50).into_iter().enumerate() {
            wheel(cx, with_layers, dy);
            wheel(cx, without_layers, dy);
            if composites(cx, with_layers) {
                composited += 1;
            }
            let expected = with_window(cx, without_layers, |window, _| {
                assert!(window.rendered_frame.scene.layers.frames.is_empty());
                expanded_quads(&window.rendered_frame.scene)
            });
            let actual = with_window(cx, with_layers, |window, _| {
                expanded_quads(&window.rendered_frame.scene)
            });
            assert_eq!(actual, expected, "frame {frame}, scrolled by {dy}");
        }
        assert!(composited > 40, "the layer was composited ({composited})");
        let extended = extended_frames() - extended;
        assert!(extended > 35, "frames kept the rows held ({extended})");
    }

    /// Scrolls a uniform list of rows `row_height` px tall at a scale of
    /// 1.25 by fractional deltas, checking it draws as it does without
    /// layers; returns in how many frames it composited its layer.
    fn uniform_list_matches_layers_off_at_a_fractional_scale(
        cx: &mut TestAppContext,
        row_height: f32,
    ) -> usize {
        let mut composited = 0;
        for seed in 0..6 {
            let (with_layers, _) = page_with_rows_of(cx, 300, row_height, 1.25);
            let (without_layers, _) = page_with_rows_of(cx, 300, row_height, 1.25);
            let with_layers: AnyWindowHandle = with_layers.into();
            let without_layers: AnyWindowHandle = without_layers.into();
            composited += compare_with_layers_off(
                cx,
                with_layers,
                without_layers,
                &fractional_deltas(seed, 120),
                &format!("rows of {row_height} px, seed {seed}"),
            );
        }
        composited
    }

    #[crate::test]
    fn uniform_list_matches_layers_off_at_a_fractional_scale_with_whole_rows(
        cx: &mut TestAppContext,
    ) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        // 20 px rows are 25 device pixels.
        let extended = extended_frames();
        let composited = uniform_list_matches_layers_off_at_a_fractional_scale(cx, ROW_HEIGHT);
        assert!(composited > 300, "the layer was composited ({composited})");
        let extended = extended_frames() - extended;
        assert!(extended > 550, "frames kept the rows held ({extended})");
    }

    #[crate::test]
    fn uniform_list_matches_layers_off_at_a_fractional_scale_with_half_pixel_rows(
        cx: &mut TestAppContext,
    ) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        // 30 px rows are 37.5 device pixels.
        let extended = extended_frames();
        let composited = uniform_list_matches_layers_off_at_a_fractional_scale(cx, 30.);
        let extended = extended_frames() - extended;
        assert!(composited > 550, "the layer was composited ({composited})");
        assert!(extended > 550, "frames kept the rows held ({extended})");
    }

    #[crate::test]
    fn measure_item_is_skipped_on_composite_frames(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (handle, log) = page(cx, 1000);
        let window = handle.into();
        let measured = |log: &Rc<RefCell<Vec<Range<usize>>>>| {
            std::mem::take(&mut *log.borrow_mut())
                .into_iter()
                .filter(|range| *range == (0..1))
                .count()
        };
        measured(&log);
        // Layout and prepaint each measure row 0.
        wheel(cx, window, -ROW_HEIGHT);
        assert_eq!(measured(&log), 2);
        promote_from_second_frame(cx, window);
        measured(&log);

        wheel(cx, window, -ROW_HEIGHT);
        assert_eq!(decision(cx, window), Some(Decision::Composite));
        assert_eq!(measured(&log), 1, "only layout measures row 0");

        let frame = with_window(cx, window, |window, _| window.fast_layers.frame);
        handle
            .update(cx, |page, _, cx| {
                page.count = 999;
                cx.notify();
            })
            .unwrap();
        if with_window(cx, window, |window, _| window.fast_layers.frame) == frame {
            draw(cx, window);
        }
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
        assert_eq!(measured(&log), 2, "a frame that changed measures again");
    }

    /// The second scrolled frame, which promotes the list.
    fn promote_from_second_frame(cx: &mut TestAppContext, window: AnyWindowHandle) {
        wheel(cx, window, -ROW_HEIGHT);
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
    }

    #[crate::test]
    fn adding_a_row_keeps_the_rows_held_as_they_were(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (handle, log) = page(cx, 1000);
        let window = handle.into();
        promote(cx, window);
        wheel(cx, window, -ROW_HEIGHT);
        let parts = |cx: &mut TestAppContext| {
            with_window(cx, window, |window, _| {
                let layer = window.fast_layers.layers.values().next().unwrap();
                let record = layer.record.as_ref().unwrap();
                let parts: Vec<_> = record.content.part_scenes().cloned().collect();
                (parts, record.dirty_tiles.clone(), record.tile_hashes.len())
            })
        };
        for step in 0..10 {
            let (before, _, _) = parts(cx);
            rendered_rows(&log);
            wheel(cx, window, -ROW_HEIGHT);
            assert_eq!(decision(cx, window), Some(Decision::Composite));
            assert_eq!(rendered_rows(&log).len(), 1, "step {step}: one row added");
            let (after, dirty, tiles) = parts(cx);
            // One part per row: those of the rows kept are the ones the layer
            // held, not copies, and only the tiles the new row reaches are
            // hashed differently.
            assert_eq!(after.len(), before.len() + 1, "step {step}");
            let kept = after
                .iter()
                .filter(|part| before.iter().any(|old| std::rc::Rc::ptr_eq(old, part)))
                .count();
            assert_eq!(kept, before.len(), "step {step}");
            assert!(
                (1..=2).contains(&dirty.len()) && dirty.len() < tiles,
                "step {step}: {} of {tiles} tiles dirty",
                dirty.len()
            );
        }
    }
}

mod list {
    use super::{
        Decision, VIEWPORT_HEIGHT, VIEWPORT_WIDTH, compare_with_layers_off, composites, decision,
        draw, expanded_quads, extended_frames, fractional_deltas, held_rows, open_at, row_color,
        wheel, wheel_deltas, with_window,
    };
    use crate::{
        AnyWindowHandle, AppContext as _, Context, Entity, IntoElement, ListAlignment, ListState,
        ParentElement as _, Render, Styled as _, TestAppContext, Window, WindowHandle, div,
        prelude::FluentBuilder as _, px, rgb,
    };
    use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

    /// How tall row `row` is: 20, 30 or 40 px.
    fn row_height(row: usize) -> f32 {
        20. + (row % 3) as f32 * 10.
    }

    /// A white panel holding a list of rows of varying heights, 100 px tall,
    /// at the top left of the window. Every row the list renders is logged.
    pub(super) struct ListPage {
        pub(super) state: ListState,
        pub(super) rendered: Rc<RefCell<Vec<usize>>>,
    }

    impl Render for ListPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let rendered = self.rendered.clone();
            div().size_full().bg(rgb(0xffffff)).child(
                crate::list(self.state.clone(), move |row, _, _| {
                    rendered.borrow_mut().push(row);
                    div()
                        .w(px(VIEWPORT_WIDTH))
                        .h(px(row_height(row)))
                        .bg(row_color(row))
                        .into_any_element()
                })
                .w(px(VIEWPORT_WIDTH))
                .h(px(VIEWPORT_HEIGHT)),
            )
        }
    }

    fn page(
        cx: &mut TestAppContext,
        state: ListState,
    ) -> (WindowHandle<ListPage>, Rc<RefCell<Vec<usize>>>) {
        page_at(cx, state, 1.)
    }

    fn page_at(
        cx: &mut TestAppContext,
        state: ListState,
        scale_factor: f32,
    ) -> (WindowHandle<ListPage>, Rc<RefCell<Vec<usize>>>) {
        let rendered = Rc::new(RefCell::new(Vec::new()));
        let log = rendered.clone();
        let window = cx.add_window(move |_, _| ListPage {
            state,
            rendered: log,
        });
        open_at(cx, window.into(), scale_factor);
        (window, rendered)
    }

    fn promote(cx: &mut TestAppContext, window: AnyWindowHandle) {
        wheel(cx, window, -20.);
        assert_eq!(decision(cx, window), Some(Decision::Bypass));
        wheel(cx, window, -20.);
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
    }

    /// The rows rendered since the log was last taken.
    fn rendered(log: &Rc<RefCell<Vec<usize>>>) -> BTreeSet<usize> {
        log.borrow_mut().drain(..).collect()
    }

    #[crate::test]
    fn list_renders_only_new_rows_when_scrolling(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let state = ListState::new(1000, ListAlignment::Top, px(0.)).measure_all();
        let (handle, log) = page(cx, state);
        let window = handle.into();
        promote(cx, window);
        let mut held: BTreeSet<usize> = held_rows(cx, window).into_iter().collect();
        assert!(held.contains(&0) && held.len() > 5, "held {held:?}");
        rendered(&log);

        let mut rendered_any = false;
        for step in 0..30 {
            wheel(cx, window, -15.);
            assert_eq!(
                decision(cx, window),
                Some(Decision::Composite),
                "step {step}"
            );
            let rendered = rendered(&log);
            let now: BTreeSet<usize> = held_rows(cx, window).into_iter().collect();
            let added: BTreeSet<usize> = now.difference(&held).copied().collect();
            assert_eq!(
                rendered, added,
                "step {step}: only the rows new to the layer are rendered"
            );
            assert!(rendered.len() <= 2, "step {step}: {rendered:?}");
            rendered_any |= !rendered.is_empty();
            held = now;
        }
        assert!(rendered_any);
        assert!(
            held.first().copied().unwrap_or(0) > 0,
            "rows left behind are dropped"
        );

        for step in 0..10 {
            wheel(cx, window, 25.);
            assert_eq!(decision(cx, window), Some(Decision::Composite), "up {step}");
            let rendered = rendered(&log);
            let now: BTreeSet<usize> = held_rows(cx, window).into_iter().collect();
            let added: BTreeSet<usize> = now.difference(&held).copied().collect();
            assert_eq!(rendered, added, "up {step}");
            held = now;
        }
    }

    #[crate::test]
    fn list_matches_layers_off_with_varying_heights(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (with_layers, _) = page(cx, ListState::new(300, ListAlignment::Top, px(50.)));
        let (without_layers, _) = page(cx, ListState::new(300, ListAlignment::Top, px(50.)));
        let with_layers: AnyWindowHandle = with_layers.into();
        let without_layers: AnyWindowHandle = without_layers.into();
        with_window(cx, without_layers, |window, _| {
            window.set_scroll_layers(false)
        });
        draw(cx, without_layers);

        let mut composited = 0;
        let extended = extended_frames();
        let mut deltas = wheel_deltas(50);
        // And back up past where it started, through rows measured on the way.
        deltas.extend([60., 60., 60., 45., 60., 60., 60., 60., 33., 60.]);
        for (frame, dy) in deltas.into_iter().enumerate() {
            wheel(cx, with_layers, dy);
            wheel(cx, without_layers, dy);
            if composites(cx, with_layers) {
                composited += 1;
            }
            let expected = with_window(cx, without_layers, |window, _| {
                assert!(window.rendered_frame.scene.layers.frames.is_empty());
                expanded_quads(&window.rendered_frame.scene)
            });
            let actual = with_window(cx, with_layers, |window, _| {
                expanded_quads(&window.rendered_frame.scene)
            });
            assert_eq!(actual, expected, "frame {frame}, scrolled by {dy}");
        }
        assert!(composited > 40, "the layer was composited ({composited})");
        let extended = extended_frames() - extended;
        assert!(extended > 40, "frames kept the rows held ({extended})");
    }

    /// Scrolls a list aligned by `alignment` with rows of 20, 30 and 40 px
    /// at a scale of 1.25, where a 30 px row is 37.5 device pixels, by
    /// fractional deltas, and checks it draws as it does without layers.
    fn list_matches_layers_off_at_a_fractional_scale(
        cx: &mut TestAppContext,
        alignment: ListAlignment,
    ) -> usize {
        let mut composited = 0;
        let mut cached_rows = 0;
        let mut direct_rows = 0;
        for seed in 0..8 {
            let (with_layers, cached_log) =
                page_at(cx, ListState::new(300, alignment, px(50.)), 1.25);
            let (without_layers, direct_log) =
                page_at(cx, ListState::new(300, alignment, px(50.)), 1.25);
            let with_layers: AnyWindowHandle = with_layers.into();
            let without_layers: AnyWindowHandle = without_layers.into();
            cached_log.borrow_mut().clear();
            direct_log.borrow_mut().clear();
            composited += compare_with_layers_off(
                cx,
                with_layers,
                without_layers,
                &fractional_deltas(seed, 120),
                &format!("{alignment:?}, seed {seed}"),
            );
            cached_rows += cached_log.borrow().len();
            direct_rows += direct_log.borrow().len();
        }
        assert!(
            cached_rows < direct_rows,
            "{alignment:?}: cached path rendered {cached_rows} rows, direct path {direct_rows}"
        );
        composited
    }

    #[crate::test]
    fn list_matches_layers_off_at_a_fractional_scale_aligned_top(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let extended = extended_frames();
        let composited = list_matches_layers_off_at_a_fractional_scale(cx, ListAlignment::Top);
        assert!(composited > 480, "the layer was composited ({composited})");
        let extended = extended_frames() - extended;
        // Expensive extensions may fall back; the differential helper also
        // checks that the cached path renders fewer rows overall.
        assert!(extended > 480, "frames kept the rows held ({extended})");
    }

    #[crate::test]
    fn list_matches_layers_off_at_a_fractional_scale_aligned_bottom(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let extended = extended_frames();
        let composited = list_matches_layers_off_at_a_fractional_scale(cx, ListAlignment::Bottom);
        assert!(composited > 480, "the layer was composited ({composited})");
        let extended = extended_frames() - extended;
        // Expensive extensions may fall back; the differential helper also
        // checks that the cached path renders fewer rows overall.
        assert!(extended > 480, "frames kept the rows held ({extended})");
    }

    #[crate::test]
    fn a_list_splice_repaints_the_layer(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let state = ListState::new(1000, ListAlignment::Top, px(0.)).measure_all();
        let (handle, _) = page(cx, state);
        let window = handle.into();
        promote(cx, window);
        wheel(cx, window, -20.);
        assert_eq!(decision(cx, window), Some(Decision::Composite));

        let frame = with_window(cx, window, |window, _| window.fast_layers.frame);
        handle
            .update(cx, |page, _, cx| {
                page.state.splice(3..4, 2);
                cx.notify();
            })
            .unwrap();
        if with_window(cx, window, |window, _| window.fast_layers.frame) == frame {
            draw(cx, window);
        }
        assert_eq!(decision(cx, window), Some(Decision::Repaint));

        wheel(cx, window, -20.);
        assert_eq!(decision(cx, window), Some(Decision::Composite));
    }

    /// A row in a view of its own, of `color`, that asks for an animation
    /// frame each time it renders when it `animate`s, and holds an anchored
    /// element when it is `anchored`.
    struct Ticker {
        animate: bool,
        anchored: bool,
        color: usize,
    }

    impl Render for Ticker {
        fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            if self.animate {
                window.request_animation_frame();
            }
            div()
                .w(px(VIEWPORT_WIDTH))
                .h(px(20.))
                .bg(row_color(self.color))
                .when(self.anchored, |row| {
                    row.child(crate::anchored().child(div().w(px(50.)).h(px(50.)).bg(crate::red())))
                })
        }
    }

    /// A list of 20 px rows whose row 2 is a [`Ticker`].
    struct TickerListPage {
        state: ListState,
        ticker: Entity<Ticker>,
    }

    impl Render for TickerListPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let ticker = self.ticker.clone();
            div().size_full().bg(rgb(0xffffff)).child(
                crate::list(self.state.clone(), move |row, _, _| {
                    if row == 2 {
                        return ticker.clone().into_any_element();
                    }
                    div()
                        .w(px(VIEWPORT_WIDTH))
                        .h(px(20.))
                        .bg(row_color(row))
                        .into_any_element()
                })
                .w(px(VIEWPORT_WIDTH))
                .h(px(VIEWPORT_HEIGHT)),
            )
        }
    }

    /// A [`TickerListPage`] scrolled until its list has a layer, the ticker
    /// showing at the top of the list.
    fn ticker_page(
        cx: &mut TestAppContext,
        animate: bool,
        anchored: bool,
    ) -> (AnyWindowHandle, Entity<Ticker>) {
        let handle = cx.add_window(move |_, cx| TickerListPage {
            state: ListState::new(300, ListAlignment::Top, px(0.)),
            ticker: cx.new(|_| Ticker {
                animate,
                anchored,
                color: 2,
            }),
        });
        let window: AnyWindowHandle = handle.into();
        draw(cx, window);
        draw(cx, window);
        let ticker = handle.update(cx, |page, _, _| page.ticker.clone()).unwrap();
        promote(cx, window);
        (window, ticker)
    }

    #[crate::test]
    fn an_animating_row_view_keeps_the_list_off_its_layer(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (window, ticker) = ticker_page(cx, true, false);
        // The ticker asks for an animation frame each time it renders, and
        // is notified each frame, as the animation frame it asked for would
        // have it: what it draws changes every frame, and the list is drawn
        // on today's path, as a div whose content view animates is.
        for step in 0..4 {
            ticker.update(cx, |_, cx| cx.notify());
            wheel(cx, window, -2.);
            assert_eq!(decision(cx, window), Some(Decision::Bypass), "step {step}");
        }
    }

    #[crate::test]
    fn a_row_view_held_by_the_layer_stays_tracked(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let (window, ticker) = ticker_page(cx, false, false);
        let tracked = |cx: &mut TestAppContext| {
            with_window(cx, window, |window, cx| {
                let id = window.window_handle().window_id();
                cx.tracked_entities
                    .get(&id)
                    .is_some_and(|tracked| tracked.contains(&ticker.entity_id()))
            })
        };
        for step in 0..3 {
            wheel(cx, window, -2.);
            assert_eq!(
                decision(cx, window),
                Some(Decision::Composite),
                "step {step}"
            );
            // On today's path the list renders the ticker, which it shows,
            // every frame: the window is told when it is notified.
            assert!(tracked(cx), "step {step}: the window tracks the ticker");
        }

        ticker.update(cx, |ticker, cx| {
            ticker.color = 50;
            cx.notify();
        });
        wheel(cx, window, -2.);
        let color = format!("{:?}", crate::Background::from(row_color(50)));
        with_window(cx, window, |window, _| {
            let quads = expanded_quads(&window.rendered_frame.scene);
            assert!(
                quads.iter().any(|quad| quad.ends_with(&color)),
                "the ticker shows its new colour"
            );
        });
    }

    #[crate::test]
    fn an_anchored_element_in_a_row_keeps_the_list_off_its_layer(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        // Positioned against the window's edges when prepainted, which a
        // composited layer would move with the rows.
        let (window, _) = ticker_page(cx, false, true);
        for step in 0..3 {
            wheel(cx, window, -2.);
            assert_eq!(decision(cx, window), Some(Decision::Bypass), "step {step}");
        }
    }
}

/// Lists whose rows hand the frame more than what they draw: hitboxes,
/// element states, paths; and rows in views of their own.
mod rows {
    use super::{
        Decision, VIEWPORT_HEIGHT, VIEWPORT_WIDTH, compare_with_layers_off, decision, draw,
        expanded_quads, held_rows, open_at, row_color, wheel, with_window,
    };
    use crate::{
        AnyWindowHandle, AppContext as _, Bounds, Context, Entity, IntoElement, ListAlignment,
        ListState, ParentElement as _, Pixels, Render, StatefulInteractiveElement as _,
        Styled as _, TestAppContext, Window, div, px, rgb,
    };
    use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

    /// What each row of a [`RowsPage`] holds besides its colour.
    #[derive(Clone, Copy)]
    enum RowKind {
        /// A scroll container of its own, taller inside than it shows.
        Scrolling,
        /// A path, drawn over the row's colour.
        Path,
        /// A hover style, as a list's rows mostly have: a hitbox.
        Hover,
    }

    /// A row of `kind`, 40 px tall.
    fn row(kind: RowKind, ix: usize) -> crate::AnyElement {
        use crate::InteractiveElement as _;
        let base = div().w(px(VIEWPORT_WIDTH)).h(px(40.)).bg(row_color(ix));
        match kind {
            RowKind::Scrolling => base
                .child(
                    div()
                        .id(("inner", ix))
                        .overflow_y_scroll()
                        .h(px(40.))
                        .child(div().h(px(40.)).bg(row_color(ix + 7)))
                        .child(div().h(px(40.)).bg(row_color(ix + 13))),
                )
                .into_any_element(),
            RowKind::Path => base
                .child(
                    crate::canvas(
                        |_, _, _| {},
                        |bounds, _, window, _| {
                            let origin = bounds.origin;
                            let mut path = crate::Path::new(origin);
                            path.line_to(origin + crate::point(px(30.), px(0.)));
                            path.line_to(origin + crate::point(px(0.), px(30.)));
                            window.paint_path(path, crate::black());
                        },
                    )
                    .h(px(40.))
                    .w_full(),
                )
                .into_any_element(),
            RowKind::Hover => base
                .id(("row", ix))
                .hover(move |style| style.bg(row_color(ix + 1)))
                .into_any_element(),
        }
    }

    /// A white panel holding a list, or a uniform list when `uniform`, of
    /// 300 rows of `kind`, 100 px tall, at the top left of the window.
    struct RowsPage {
        kind: RowKind,
        uniform: bool,
        state: ListState,
    }

    impl Render for RowsPage {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let kind = self.kind;
            let page = div().size_full().bg(rgb(0xffffff));
            if self.uniform {
                page.child(
                    crate::uniform_list(
                        "list",
                        300,
                        cx.processor(move |_, range: std::ops::Range<usize>, _, _| {
                            range.map(|ix| row(kind, ix)).collect::<Vec<_>>()
                        }),
                    )
                    .w(px(VIEWPORT_WIDTH))
                    .h(px(VIEWPORT_HEIGHT)),
                )
            } else {
                page.child(
                    crate::list(self.state.clone(), move |ix, _, _| row(kind, ix))
                        .w(px(VIEWPORT_WIDTH))
                        .h(px(VIEWPORT_HEIGHT)),
                )
            }
        }
    }

    fn page(cx: &mut TestAppContext, kind: RowKind, uniform: bool) -> AnyWindowHandle {
        let window = cx.add_window(move |_, _| RowsPage {
            kind,
            uniform,
            state: ListState::new(300, ListAlignment::Top, px(0.)).measure_all(),
        });
        open_at(cx, window.into(), 1.);
        window.into()
    }

    /// The part of each hitbox of the window's last frame that can be hit,
    /// sorted.
    fn hittable(cx: &mut TestAppContext, window: AnyWindowHandle) -> Vec<Bounds<Pixels>> {
        with_window(cx, window, |window, _| {
            let mut hittable: Vec<_> = window
                .rendered_frame
                .hitboxes
                .iter()
                .map(|hitbox| hitbox.bounds.intersect(&hitbox.content_mask.bounds))
                .filter(|bounds| !bounds.is_empty())
                .collect();
            hittable.sort_by_key(|bounds| format!("{bounds:?}"));
            hittable
        })
    }

    /// Scrolls a list whose rows are scroll containers themselves, and checks
    /// that it can be hit, and draws, as it does without layers: a wheel over
    /// a row the layer holds scrolls that row.
    fn rows_that_can_be_hit_stay_hittable(cx: &mut TestAppContext, uniform: bool) {
        let with_layers = page(cx, RowKind::Scrolling, uniform);
        let without_layers = page(cx, RowKind::Scrolling, uniform);
        with_window(cx, without_layers, |window, _| {
            window.set_scroll_layers(false)
        });
        draw(cx, without_layers);
        // Each wheel scrolls the row under the pointer as far as it goes, then
        // the list.
        for (frame, dy) in [-40., -40., -40., -40., -10., -40., 25., -40., -40., -40.]
            .into_iter()
            .enumerate()
        {
            wheel(cx, with_layers, dy);
            wheel(cx, without_layers, dy);
            assert_eq!(
                hittable(cx, with_layers),
                hittable(cx, without_layers),
                "frame {frame}: the rows can be hit as without layers"
            );
            let expected = with_window(cx, without_layers, |window, _| {
                expanded_quads(&window.rendered_frame.scene)
            });
            let actual = with_window(cx, with_layers, |window, _| {
                expanded_quads(&window.rendered_frame.scene)
            });
            assert_eq!(actual, expected, "frame {frame}, scrolled by {dy}");
        }
    }

    #[crate::test]
    fn a_list_whose_rows_can_be_hit_stays_hittable(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        rows_that_can_be_hit_stay_hittable(cx, false);
    }

    #[crate::test]
    fn a_uniform_list_whose_rows_can_be_hit_stays_hittable(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        rows_that_can_be_hit_stay_hittable(cx, true);
    }

    #[crate::test]
    fn a_list_whose_rows_paint_paths_is_kept_off_its_layer(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        for uniform in [false, true] {
            let with_layers = page(cx, RowKind::Path, uniform);
            let without_layers = page(cx, RowKind::Path, uniform);
            // Promoted and painted once into the layer, then demoted: paths
            // composited from tiles would not land as drawn afresh.
            compare_with_layers_off(cx, with_layers, without_layers, &[-20., -20.], "promote");
            assert_eq!(decision(cx, with_layers), Some(Decision::Repaint));
            for step in 0..5 {
                compare_with_layers_off(cx, with_layers, without_layers, &[-15.], "demoted");
                assert_eq!(
                    decision(cx, with_layers),
                    Some(Decision::Bypass),
                    "uniform {uniform}, step {step}"
                );
            }
        }
    }

    /// A list whose rows take input (a hover style: a hitbox, a listener,
    /// element state) is composited from its layer while it scrolls under a
    /// still pointer, its rows' records carried from frame to frame; the
    /// rows whose hover changes, and only those, are rendered again, and it
    /// draws and can be hit as without layers.
    #[crate::test]
    fn a_list_whose_rows_take_input_composites_its_layer(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        for uniform in [false, true] {
            let with_layers = page(cx, RowKind::Hover, uniform);
            let without_layers = page(cx, RowKind::Hover, uniform);
            with_window(cx, with_layers, |window, _| window.reset_layout_stats());
            let extended = crate::fast::layers::lists::extended_frames();
            let for_hover = crate::fast::layers::lists::rows_rendered_for_hover();
            let mut composited = 0;
            for sweep in 0..3 {
                let dy = if sweep % 2 == 0 { -5. } else { 5. };
                composited +=
                    compare_with_layers_off(cx, with_layers, without_layers, &[dy; 80], "scroll");
                assert_eq!(
                    hittable(cx, with_layers),
                    hittable(cx, without_layers),
                    "uniform {uniform}, sweep {sweep}"
                );
            }
            let extended = crate::fast::layers::lists::extended_frames() - extended;
            let for_hover = crate::fast::layers::lists::rows_rendered_for_hover() - for_hover;
            let stats = with_window(cx, with_layers, |window, _| window.layout_stats());
            assert!(
                composited >= 235,
                "uniform {uniform}: composited {composited}"
            );
            assert!(
                extended >= 235,
                "uniform {uniform}: kept rows in {extended}"
            );
            assert_eq!(stats.layers_demoted, 0, "uniform {uniform}");
            // The pointer, near the top of the list, crosses into another row
            // every eight frames: the row it leaves and the row it enters are
            // rendered again, and so is a row showing again after it left.
            assert!(
                (50..=90).contains(&for_hover),
                "uniform {uniform}: {for_hover} rows rendered again for their hover"
            );
        }
    }

    /// A white panel holding a uniform list of 300 rows with a hover style,
    /// 40 px tall, 100 px tall itself, at the top left of the window. Every
    /// range of rows the list renders is logged.
    struct HoverRowsPage {
        rendered: Rc<RefCell<Vec<std::ops::Range<usize>>>>,
    }

    impl Render for HoverRowsPage {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let rendered = self.rendered.clone();
            div().size_full().bg(rgb(0xffffff)).child(
                crate::uniform_list(
                    "list",
                    300,
                    cx.processor(move |_, range: std::ops::Range<usize>, _, _| {
                        rendered.borrow_mut().push(range.clone());
                        range.map(|ix| row(RowKind::Hover, ix)).collect::<Vec<_>>()
                    }),
                )
                .w(px(VIEWPORT_WIDTH))
                .h(px(VIEWPORT_HEIGHT)),
            )
        }
    }

    /// Rows move under a still pointer: a frame renders the rows new to the
    /// layer and, when the pointer crosses into another row, the row it left
    /// and the row it entered, whose hover styles change, and no other.
    #[crate::test]
    fn a_hover_change_renders_only_the_rows_it_touches(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let rendered = Rc::new(RefCell::new(Vec::new()));
        let log = rendered.clone();
        let window: AnyWindowHandle = cx
            .add_window(move |_, _| HoverRowsPage { rendered: log })
            .into();
        open_at(cx, window, 1.);
        // Far enough down that row 0, which the list renders alone to measure
        // it, is not held.
        for _ in 0..40 {
            wheel(cx, window, -40.);
        }
        let step = 5.;
        wheel(cx, window, -step);
        assert_eq!(decision(cx, window), Some(Decision::Composite));
        // The wheel is turned with the pointer 20 px down the list.
        let hovered = |offset: f32| ((20. + offset) / 40.).floor() as usize;
        let mut offset = 1600. + step;
        let mut held: BTreeSet<usize> = held_rows(cx, window).into_iter().collect();
        let mut crossings = 0;
        rendered.borrow_mut().clear();
        for frame in 0..60 {
            let before = hovered(offset);
            offset += step;
            wheel(cx, window, -step);
            assert_eq!(
                decision(cx, window),
                Some(Decision::Composite),
                "frame {frame}"
            );
            let now: BTreeSet<usize> = held_rows(cx, window).into_iter().collect();
            let rows: BTreeSet<usize> = rendered
                .borrow_mut()
                .drain(..)
                .filter(|range| *range != (0..1))
                .flatten()
                .collect();
            let again: BTreeSet<usize> = rows.intersection(&held).copied().collect();
            let expected: BTreeSet<usize> = if hovered(offset) == before {
                BTreeSet::new()
            } else {
                crossings += 1;
                [before, hovered(offset)].into()
            };
            assert_eq!(again, expected, "frame {frame}: rows rendered again");
            assert!(rows.len() <= 3, "frame {frame}: {rows:?}");
            held = now;
        }
        assert!(crossings >= 7, "the pointer crossed {crossings} rows");
    }

    /// A row in a view of its own, of `color`.
    struct ColorRow {
        color: usize,
    }

    impl Render for ColorRow {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .w(px(VIEWPORT_WIDTH))
                .h(px(20.))
                .bg(row_color(self.color))
        }
    }

    /// A list of 20 px rows whose row 2 is a [`ColorRow`].
    struct ViewRowPage {
        state: ListState,
        view: Entity<ColorRow>,
    }

    impl Render for ViewRowPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let view = self.view.clone();
            div().size_full().bg(rgb(0xffffff)).child(
                crate::list(self.state.clone(), move |ix, _, _| {
                    if ix == 2 {
                        return view.clone().into_any_element();
                    }
                    div()
                        .w(px(VIEWPORT_WIDTH))
                        .h(px(20.))
                        .bg(row_color(ix))
                        .into_any_element()
                })
                .w(px(VIEWPORT_WIDTH))
                .h(px(VIEWPORT_HEIGHT)),
            )
        }
    }

    /// A list of 300 rows, each a [`ColorRow`] of its own, beside an anchored
    /// element when `anchored`.
    struct ViewRowsPage {
        state: ListState,
        views: Vec<Entity<ColorRow>>,
        anchored: bool,
    }

    impl Render for ViewRowsPage {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            use crate::prelude::FluentBuilder as _;
            let views = self.views.clone();
            div()
                .size_full()
                .bg(rgb(0xffffff))
                .child(
                    crate::list(self.state.clone(), move |ix, _, _| {
                        views[ix].clone().into_any_element()
                    })
                    .w(px(VIEWPORT_WIDTH))
                    .h(px(VIEWPORT_HEIGHT)),
                )
                .when(self.anchored, |page| {
                    page.child(
                        crate::anchored().child(div().w(px(50.)).h(px(50.)).bg(crate::red())),
                    )
                })
        }
    }

    fn view_rows_page(cx: &mut TestAppContext, anchored: bool) -> AnyWindowHandle {
        let handle = cx.add_window(move |_, cx| ViewRowsPage {
            state: ListState::new(300, ListAlignment::Top, px(0.)).measure_all(),
            views: (0..300)
                .map(|color| cx.new(|_| ColorRow { color }))
                .collect(),
            anchored,
        });
        let window: AnyWindowHandle = handle.into();
        open_at(cx, window, 1.);
        window
    }

    #[crate::test]
    fn an_anchored_element_beside_a_list_leaves_it_on_its_layer(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = view_rows_page(cx, true);
        let extended = crate::fast::layers::lists::extended_frames();
        for _ in 0..2 {
            wheel(cx, window, -10.);
        }
        assert_eq!(decision(cx, window), Some(Decision::Repaint));
        for step in 0..4 {
            wheel(cx, window, -10.);
            assert_eq!(
                decision(cx, window),
                Some(Decision::Composite),
                "step {step}"
            );
        }
        assert_eq!(crate::fast::layers::lists::extended_frames(), extended + 4);
    }

    #[crate::test]
    fn what_a_list_layer_remembers_of_its_rows_stays_bounded(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let window = view_rows_page(cx, false);
        for _ in 0..2 {
            wheel(cx, window, -10.);
        }
        let extended = crate::fast::layers::lists::extended_frames();
        // Down through most of the 300 rows, 15 px a frame.
        for step in 0..350 {
            wheel(cx, window, -15.);
            assert_eq!(
                decision(cx, window),
                Some(Decision::Composite),
                "step {step}"
            );
            with_window(cx, window, |window, _| {
                let layer = window.fast_layers.layers.values().next().unwrap();
                let views = layer.record.as_ref().unwrap().views.len();
                let held = layer.rows.held().count();
                assert!(
                    views <= held * 6,
                    "step {step}: the layer remembers {views} views for {held} rows"
                );
            });
        }
        let extended = crate::fast::layers::lists::extended_frames() - extended;
        assert!(extended > 300, "frames kept the rows held ({extended})");
    }

    #[crate::test]
    fn a_row_view_held_by_the_layer_shows_a_change_without_a_scroll(cx: &mut TestAppContext) {
        if !crate::fast::layers::COMPILED {
            return;
        }
        let handle = cx.add_window(move |_, cx| ViewRowPage {
            state: ListState::new(300, ListAlignment::Top, px(0.)),
            view: cx.new(|_| ColorRow { color: 2 }),
        });
        let window: AnyWindowHandle = handle.into();
        open_at(cx, window, 1.);
        let view = handle.update(cx, |page, _, _| page.view.clone()).unwrap();
        for _ in 0..4 {
            wheel(cx, window, -2.);
        }
        assert_eq!(decision(cx, window), Some(Decision::Composite));

        view.update(cx, |row, cx| {
            row.color = 50;
            cx.notify();
        });
        draw(cx, window);
        let color = format!("{:?}", crate::Background::from(row_color(50)));
        with_window(cx, window, |window, _| {
            let quads = expanded_quads(&window.rendered_frame.scene);
            assert!(
                quads.iter().any(|quad| quad.ends_with(&color)),
                "the row view shows its new colour"
            );
        });
    }
}
