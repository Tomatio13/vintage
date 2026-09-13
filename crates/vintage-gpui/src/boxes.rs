//! Full-cell vector drawings for the characters whose lines must stay
//! seamless regardless of font coverage: box drawing, block elements, and the
//! four powerline triangles/arrows. Fonts often render these glyphs slightly
//! narrower than the cell, which leaves gaps and misaligned weights in TUI
//! layouts, so like Alacritty we draw them ourselves.
//!
//! Geometry is computed in whole physical pixels and converted to logical
//! pixels, so strokes stay crisp and meet neighboring cells exactly.

use gpui::Pixels;

/// A stroked polyline in cell-local logical pixels.
#[derive(Clone, Debug, Default)]
pub struct Stroke {
    pub points: Vec<[f32; 2]>,
    pub width: f32,
}

/// Vector art for one cell, in cell-local logical pixels.
#[derive(Clone, Debug, Default)]
pub struct Glyph {
    pub rects: Vec<[f32; 4]>,
    /// Rectangles blended over the background with the given alpha, used by
    /// the shade blocks that fonts usually dither.
    pub tints: Vec<([f32; 4], u8)>,
    pub strokes: Vec<Stroke>,
    pub polygons: Vec<Vec<[f32; 2]>>,
}

impl Glyph {
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
            && self.tints.is_empty()
            && self.strokes.is_empty()
            && self.polygons.is_empty()
    }
}

/// Whether this character is drawn by [`glyph`] instead of the font.
pub fn is_drawn(ch: char) -> bool {
    let code = ch as u32;
    (0x2500..=0x259F).contains(&code) || (0xE0B0..=0xE0B3).contains(&code)
}

/// Build the full-cell drawing for a supported character, or `None` when the
/// font should render it.
pub fn glyph(ch: char, cell_width: Pixels, line_height: Pixels, scale: f32) -> Option<Glyph> {
    if scale <= 0. || !is_drawn(ch) {
        return None;
    }
    let w = (f32::from(cell_width) * scale).round().max(1.);
    let h = (f32::from(line_height) * scale).round().max(1.);
    // Light stroke matches Alacritty's builtin font: one eighth of the cell
    // width; heavy lines use double the stroke.
    let t = ((w / 8.).round() as i32).max(1) as f32;
    let mut g = match ch as u32 {
        0x2500..=0x2503 | 0x250C..=0x254B | 0x2574..=0x257F => line_drawing(ch as u32, w, h, t),
        0x2504..=0x250B | 0x254C..=0x254F => dashes(ch as u32, w, h, t),
        0x2550..=0x256C => double_lines(ch as u32, w, h, t),
        0x256D..=0x2570 => arcs(ch as u32, w, h, t),
        0x2571..=0x2573 => diagonals(ch as u32, w, h, t),
        0x2580..=0x259F => blocks(ch as u32, w, h),
        0xE0B0..=0xE0B3 => powerline(ch as u32, w, h, t)?,
        _ => return None,
    };
    if g.is_empty() {
        return None;
    }
    for rect in g.rects.iter_mut().chain(g.tints.iter_mut().map(|(r, _)| r)) {
        rect[0] /= scale;
        rect[1] /= scale;
        rect[2] /= scale;
        rect[3] /= scale;
    }
    for stroke in g.strokes.iter_mut() {
        stroke.width /= scale;
        for point in stroke.points.iter_mut() {
            point[0] /= scale;
            point[1] /= scale;
        }
    }
    for polygon in g.polygons.iter_mut() {
        for point in polygon.iter_mut() {
            point[0] /= scale;
            point[1] /= scale;
        }
    }
    Some(g)
}

/// Accumulates axis-aligned strokes in physical pixels and clamps them to the
/// cell, mirroring the raster canvas Alacritty draws onto.
#[derive(Clone, Copy)]
struct Geometry {
    w: f32,
    h: f32,
    t: f32,
}

impl Geometry {
    fn x_center(&self) -> f32 {
        (self.w / 2.).floor()
    }
    fn y_center(&self) -> f32 {
        (self.h / 2.).floor()
    }
    /// Bounds of a vertical stroke centered on `x` as `(left, right)`, with
    /// Alacritty's truncating integer offset.
    fn v_bounds(&self, x: f32, stroke: f32) -> (f32, f32) {
        let left = (x as i32 - stroke as i32 / 2) as f32;
        (left, left + stroke)
    }
    /// Bounds of a horizontal stroke centered on `y` as `(top, bottom)`.
    fn h_bounds(&self, y: f32, stroke: f32) -> (f32, f32) {
        let top = (y as i32 - stroke as i32 / 2) as f32;
        (top, top + stroke)
    }
    fn push_rect(&self, out: &mut Vec<[f32; 4]>, x: f32, y: f32, width: f32, height: f32) {
        if width <= 0. || height <= 0. {
            return;
        }
        let left = x.max(0.);
        let top = y.max(0.);
        let right = (x + width).min(self.w);
        let bottom = (y + height).min(self.h);
        if right > left && bottom > top {
            out.push([left, top, right - left, bottom - top]);
        }
    }
    fn h_line(&self, out: &mut Vec<[f32; 4]>, x: f32, y: f32, len: f32, stroke: f32) {
        if stroke <= 0. || len <= 0. {
            return;
        }
        let (top, _) = self.h_bounds(y, stroke);
        self.push_rect(out, x, top, len, stroke);
    }
    fn v_line(&self, out: &mut Vec<[f32; 4]>, x: f32, y: f32, len: f32, stroke: f32) {
        if stroke <= 0. || len <= 0. {
            return;
        }
        let (left, _) = self.v_bounds(x, stroke);
        self.push_rect(out, left, y, stroke, len);
    }
    /// One horizontal/vertical line running through the whole cell.
    fn full(&self, out: &mut Vec<[f32; 4]>, horizontal: bool, heavy: bool) {
        let stroke = if heavy { self.t * 2. } else { self.t };
        if horizontal {
            self.h_line(out, 0., self.y_center(), self.w, stroke);
        } else {
            self.v_line(out, self.x_center(), 0., self.h, stroke);
        }
    }
}

/// Straight lines, junctions, halves, and mixed-weight lines.
///
/// Arm weights follow the Unicode names for U+2500–U+2503, U+250C–U+254B and
/// U+2574–U+257F, as tabulated by Alacritty's builtin font.
fn line_drawing(code: u32, w: f32, h: f32, t: f32) -> Glyph {
    let g = Geometry { w, h, t };
    let light = t;
    let heavy = t * 2.;
    let mut rects = Vec::new();
    if (0x2500..=0x2503).contains(&code) {
        g.full(&mut rects, code <= 0x2501, code % 2 == 1);
        return Glyph {
            rects,
            ..Glyph::default()
        };
    }
    // Left horizontal arm.
    let s_h1 = match code {
        0x2500 | 0x2510 | 0x2512 | 0x2518 | 0x251A | 0x2524 | 0x2526 | 0x2527 | 0x2528 | 0x252C
        | 0x252E | 0x2530 | 0x2532 | 0x2534 | 0x2536 | 0x2538 | 0x253A | 0x253C | 0x253E
        | 0x2540 | 0x2541 | 0x2542 | 0x2544 | 0x2546 | 0x254A | 0x2574 | 0x257C => light,
        0x2501 | 0x2511 | 0x2513 | 0x2519 | 0x251B | 0x2525 | 0x2529 | 0x252A | 0x252B | 0x252D
        | 0x252F | 0x2531 | 0x2533 | 0x2535 | 0x2537 | 0x2539 | 0x253B | 0x253D | 0x253F
        | 0x2543 | 0x2545 | 0x2547 | 0x2548 | 0x2549 | 0x254B | 0x2578 | 0x257E => heavy,
        _ => 0.,
    };
    // Right horizontal arm.
    let s_h2 = match code {
        0x2500 | 0x250C | 0x250E | 0x2514 | 0x2516 | 0x251C | 0x251E | 0x251F | 0x2520 | 0x252C
        | 0x252D | 0x2530 | 0x2531 | 0x2534 | 0x2535 | 0x2538 | 0x2539 | 0x253C | 0x253D
        | 0x2540 | 0x2541 | 0x2542 | 0x2543 | 0x2545 | 0x2549 | 0x2576 | 0x257E => light,
        0x2501 | 0x250D | 0x250F | 0x2515 | 0x2517 | 0x251D | 0x2521 | 0x2522 | 0x2523 | 0x252E
        | 0x252F | 0x2532 | 0x2533 | 0x2536 | 0x2537 | 0x253A | 0x253B | 0x253E | 0x253F
        | 0x2544 | 0x2546 | 0x2547 | 0x2548 | 0x254A | 0x254B | 0x257A | 0x257C => heavy,
        _ => 0.,
    };
    // Top vertical arm.
    let s_v1 = match code {
        0x2502 | 0x2514 | 0x2515 | 0x2518 | 0x2519 | 0x251C | 0x251D | 0x251F | 0x2522 | 0x2524
        | 0x2525 | 0x2527 | 0x252A | 0x2534 | 0x2535 | 0x2536 | 0x2537 | 0x253C | 0x253D
        | 0x253E | 0x253F | 0x2541 | 0x2545 | 0x2546 | 0x2548 | 0x2575 | 0x257D => light,
        0x2503 | 0x2516 | 0x2517 | 0x251A | 0x251B | 0x251E | 0x2520 | 0x2521 | 0x2523 | 0x2526
        | 0x2528 | 0x2529 | 0x252B | 0x2538 | 0x2539 | 0x253A | 0x253B | 0x2540 | 0x2542
        | 0x2543 | 0x2544 | 0x2547 | 0x2549 | 0x254A | 0x254B | 0x2579 | 0x257F => heavy,
        _ => 0.,
    };
    // Bottom vertical arm.
    let s_v2 = match code {
        0x2502 | 0x250C | 0x250D | 0x2510 | 0x2511 | 0x251C | 0x251D | 0x251E | 0x2521 | 0x2524
        | 0x2525 | 0x2526 | 0x2529 | 0x252C | 0x252D | 0x252E | 0x252F | 0x253C | 0x253D
        | 0x253E | 0x253F | 0x2540 | 0x2543 | 0x2544 | 0x2547 | 0x2577 | 0x257F => light,
        0x2503 | 0x250E | 0x250F | 0x2512 | 0x2513 | 0x251F | 0x2520 | 0x2522 | 0x2523 | 0x2527
        | 0x2528 | 0x252A | 0x252B | 0x2530 | 0x2531 | 0x2532 | 0x2533 | 0x2541 | 0x2542
        | 0x2545 | 0x2546 | 0x2548 | 0x2549 | 0x254A | 0x254B | 0x257B | 0x257D => heavy,
        _ => 0.,
    };

    let x_v = g.x_center();
    let y_h = g.y_center();
    // Arms join where the crossing strokes reach; equal arms merge into one
    // full stroke, and mixed weights overlap by a pixel to avoid a seam.
    let v1 = g.v_bounds(x_v, s_v1);
    let v2 = g.v_bounds(x_v, s_v2);
    let h1 = g.h_bounds(y_h, s_h1);
    let h2 = g.h_bounds(y_h, s_h2);
    let left_end = v1.1.max(v2.1);
    let right_start = v1.0.min(v2.0);
    let top_end = h1.1.max(h2.1);
    let bottom_start = h1.0.min(h2.0);
    if s_h1 > 0. || s_h2 > 0. {
        if s_h1 == s_h2 {
            g.h_line(&mut rects, 0., y_h, w, s_h1);
        } else if left_end + 1. >= right_start {
            g.h_line(&mut rects, 0., y_h, left_end + 1., s_h1);
            g.h_line(&mut rects, (right_start - 1.).max(0.), y_h, w, s_h2);
        } else {
            g.h_line(&mut rects, 0., y_h, left_end, s_h1);
            g.h_line(&mut rects, right_start, y_h, w, s_h2);
        }
    }
    if s_v1 > 0. || s_v2 > 0. {
        if s_v1 == s_v2 {
            g.v_line(&mut rects, x_v, 0., h, s_v1);
        } else if top_end + 1. >= bottom_start {
            g.v_line(&mut rects, x_v, 0., top_end + 1., s_v1);
            g.v_line(&mut rects, x_v, (bottom_start - 1.).max(0.), h, s_v2);
        } else {
            g.v_line(&mut rects, x_v, 0., top_end, s_v1);
            g.v_line(&mut rects, x_v, bottom_start, h, s_v2);
        }
    }
    Glyph {
        rects,
        ..Glyph::default()
    }
}

/// Horizontal and vertical dashes: U+2504–U+250B and U+254C–U+254F.
fn dashes(code: u32, w: f32, h: f32, t: f32) -> Glyph {
    let g = Geometry { w, h, t };
    let heavy = matches!(code, 0x2505 | 0x2507 | 0x2509 | 0x250B | 0x254D | 0x254F);
    let stroke = if heavy { t * 2. } else { t };
    let (horizontal, num_gaps) = match code {
        0x2504 | 0x2505 => (true, 2),
        0x2506 | 0x2507 => (false, 2),
        0x2508 | 0x2509 => (true, 3),
        0x250A | 0x250B => (false, 3),
        0x254C | 0x254D => (true, 1),
        _ => (false, 1),
    };
    let mut rects = Vec::new();
    let extent = if horizontal { w } else { h };
    let gap = (extent / 8.).floor().max(1.);
    let dash = ((extent - gap * num_gaps as f32) / (num_gaps as f32 + 1.))
        .floor()
        .max(1.);
    for gap_index in 0..=num_gaps {
        let start = (gap_index as f32 * (dash + gap)).min(extent);
        if horizontal {
            g.h_line(&mut rects, start, g.y_center(), dash, stroke);
        } else {
            g.v_line(&mut rects, g.x_center(), start, dash, stroke);
        }
    }
    Glyph {
        rects,
        ..Glyph::default()
    }
}

/// Double-line components U+2550–U+256C, laid out like Alacritty's builtin
/// font: the two parallel strokes sit one pixel outside the light stroke
/// bounds around the center.
fn double_lines(code: u32, w: f32, h: f32, t: f32) -> Glyph {
    let g = Geometry { w, h, t };
    let x_c = g.x_center();
    let y_c = g.y_center();
    // Chars whose horizontal arms stay a single center line and whose
    // vertical arms stay a single center line, respectively.
    let single_v = matches!(
        code,
        0x2552 | 0x2555 | 0x2558 | 0x255B | 0x255E | 0x2561 | 0x2564 | 0x2567 | 0x256A
    );
    let single_h = matches!(
        code,
        0x2553 | 0x2556 | 0x2559 | 0x255C | 0x255F | 0x2562 | 0x2565 | 0x2568 | 0x256B
    );
    let v_lines = if single_v {
        (x_c, x_c)
    } else {
        let bounds = g.v_bounds(x_c, t);
        ((bounds.0 - 1.).max(0.), (bounds.1 + 1.).min(w))
    };
    let h_lines = if single_h {
        (y_c, y_c)
    } else {
        let bounds = g.h_bounds(y_c, t);
        ((bounds.0 - 1.).max(0.), (bounds.1 + 1.).min(h))
    };
    let v_left = g.v_bounds(v_lines.0, t);
    let v_right = g.v_bounds(v_lines.1, t);
    let h_top = g.h_bounds(h_lines.0, t);
    let h_bot = g.h_bounds(h_lines.1, t);

    // Left horizontal strokes: length from the left edge.
    let (top_left_len, bot_left_len) = match code {
        0x2550 | 0x256B => (x_c, x_c),
        0x2555..=0x2557 => (v_right.1, v_left.1),
        0x255B..=0x255D => (v_left.1, v_right.1),
        0x2561..=0x2563 | 0x256A | 0x256C => (v_left.1, v_left.1),
        0x2564..=0x2568 => (x_c, v_left.1),
        0x2569 => (v_left.1, x_c),
        _ => (0., 0.),
    };
    // Right horizontal strokes: start x and full length (clamped at the edge).
    let (top_right_x, bot_right_x, right_len) = match code {
        0x2550 | 0x2565 | 0x256B => (x_c, x_c, w),
        0x2552..=0x2554 | 0x2568 => (v_left.0, v_right.0, w),
        0x2558..=0x255A => (v_right.0, v_left.0, w),
        0x255E..=0x2560 | 0x256A | 0x256C => (v_right.0, v_right.0, w),
        0x2564 | 0x2566 => (x_c, v_right.0, w),
        0x2567 | 0x2569 => (v_right.0, x_c, w),
        _ => (0., 0., 0.),
    };
    // Top vertical strokes: length from the top edge.
    let (left_top_len, right_top_len) = match code {
        0x2551 | 0x256A => (y_c, y_c),
        0x2558..=0x255C | 0x2568 => (h_bot.1, h_top.1),
        0x255D => (h_top.1, h_bot.1),
        0x255E..=0x2560 => (y_c, h_top.1),
        0x2561..=0x2563 => (h_top.1, y_c),
        0x2567 | 0x2569 | 0x256B | 0x256C => (h_top.1, h_top.1),
        _ => (0., 0.),
    };
    // Bottom vertical strokes: start y and full length (clamped at the edge).
    let (left_bot_y, right_bot_y, bot_len) = match code {
        0x2551 | 0x256A => (y_c, y_c, h),
        0x2552..=0x2554 => (h_top.0, h_bot.0, h),
        0x2555..=0x2557 => (h_bot.0, h_top.0, h),
        0x255E..=0x2560 => (y_c, h_bot.0, h),
        0x2561..=0x2563 => (h_bot.0, y_c, h),
        0x2564..=0x2566 | 0x256B | 0x256C => (h_bot.0, h_bot.0, h),
        _ => (0., 0., 0.),
    };

    let mut rects = Vec::new();
    g.h_line(&mut rects, 0., h_lines.0, top_left_len, t);
    g.h_line(&mut rects, 0., h_lines.1, bot_left_len, t);
    g.h_line(&mut rects, top_right_x, h_lines.0, right_len, t);
    g.h_line(&mut rects, bot_right_x, h_lines.1, right_len, t);
    g.v_line(&mut rects, v_lines.0, 0., left_top_len, t);
    g.v_line(&mut rects, v_lines.1, 0., right_top_len, t);
    g.v_line(&mut rects, v_lines.0, left_bot_y, bot_len, t);
    g.v_line(&mut rects, v_lines.1, right_bot_y, bot_len, t);
    Glyph {
        rects,
        ..Glyph::default()
    }
}

/// Rounded corners ╭ ╮ ╯ ╰. Base orientation is ╯ (up and left): a stroke
/// entering from the top edge turns through a quarter circle out the left
/// edge midpoint; the others mirror that around the cell.
fn arcs(code: u32, w: f32, h: f32, t: f32) -> Glyph {
    let radius = (w.min(h) + t) / 2.;
    let mirror_x = matches!(code, 0x256D | 0x2570);
    let mirror_y = matches!(code, 0x256D | 0x256E);
    let sx = if mirror_x { -1. } else { 1. };
    let sy = if mirror_y { -1. } else { 1. };
    let entry_x = (w / 2.).floor();
    let exit = (if mirror_x { w } else { 0. }, h / 2.);
    // Circle center sits on the exit edge so the arc meets the neighboring
    // horizontal line exactly at the edge midpoint.
    let center = (exit.0, exit.1 - sy * radius);
    let straight = (if mirror_y { h - center.1 } else { center.1 }).max(0.);
    let start = (entry_x, center.1.max(0.));
    // Cubic control points for a quarter circle (kappa approximation).
    let k = 0.552_284_7;
    let ctrl_a = (start.0, start.1 + sy * k * radius);
    let ctrl_b = (center.0 + sx * k * radius, exit.1);
    let mut rects = Vec::new();
    Geometry { w, h, t }.v_line(
        &mut rects,
        entry_x,
        if mirror_y { h - straight } else { 0. },
        straight,
        t,
    );
    Glyph {
        rects,
        strokes: vec![Stroke {
            points: vec![start, ctrl_a, ctrl_b, exit]
                .into_iter()
                .map(|p| [p.0, p.1])
                .collect(),
            width: t,
        }],
        ..Glyph::default()
    }
}

/// Diagonals ╱ ╲ ╳, extended past the corners so neighboring diagonal cells
/// connect like Alacritty's builtin font.
fn diagonals(code: u32, w: f32, h: f32, t: f32) -> Glyph {
    let extend = t * 2.;
    let down = vec![[-extend, -extend], [w + extend, h + extend]];
    let up = vec![[-extend, h + extend], [w + extend, -extend]];
    let strokes = match code {
        0x2571 => vec![Stroke {
            points: up,
            width: t,
        }],
        0x2572 => vec![Stroke {
            points: down,
            width: t,
        }],
        _ => vec![
            Stroke {
                points: up,
                width: t,
            },
            Stroke {
                points: down,
                width: t,
            },
        ],
    };
    Glyph {
        rects: Vec::new(),
        tints: Vec::new(),
        strokes,
        polygons: Vec::new(),
    }
}

/// Block elements U+2580–U+259F as eighth-cell fractions.
fn blocks(code: u32, w: f32, h: f32) -> Glyph {
    let mut glyph = Glyph::default();
    let eighth_h = h / 8.;
    let eighth_w = w / 8.;
    match code {
        // Shades: blend the foreground over the background.
        0x2591..=0x2593 => {
            let alpha = match code {
                0x2591 => 0x40,
                0x2592 => 0x80,
                _ => 0xC0,
            };
            glyph.tints.push(([0., 0., w, h], alpha));
        }
        // Full and partial blocks.
        0x2580..=0x2590 | 0x2594 | 0x2595 => {
            let (x, width) = match code {
                0x2589 => (0., eighth_w * 7.),
                0x258A => (0., eighth_w * 6.),
                0x258B => (0., eighth_w * 5.),
                0x258C => (0., eighth_w * 4.),
                0x258D => (0., eighth_w * 3.),
                0x258E => (0., eighth_w * 2.),
                0x258F => (0., eighth_w),
                0x2590 => (eighth_w * 4., eighth_w * 4.),
                0x2595 => (eighth_w * 7., eighth_w),
                _ => (0., w),
            };
            let (y, height) = match code {
                0x2580 => (0., eighth_h * 4.),
                0x2581 => (eighth_h * 7., eighth_h),
                0x2582 => (eighth_h * 6., eighth_h * 2.),
                0x2583 => (eighth_h * 5., eighth_h * 3.),
                0x2584 => (eighth_h * 4., eighth_h * 4.),
                0x2585 => (eighth_h * 3., eighth_h * 5.),
                0x2586 => (eighth_h * 2., eighth_h * 6.),
                0x2587 => (eighth_h, eighth_h * 7.),
                0x2594 => (0., eighth_h),
                _ => (0., h),
            };
            glyph.rects.push([x, y, width, height]);
        }
        // Quadrants: upper-left, upper-right, lower-left, lower-right bits.
        _ => {
            let x_mid = (w / 2.).round().max(1.);
            let y_mid = (h / 2.).round().max(1.);
            let upper_left = matches!(code, 0x2598..=0x259C);
            let upper_right = matches!(code, 0x259B..=0x259F);
            let lower_left = matches!(code, 0x2596 | 0x2599 | 0x259B | 0x259E | 0x259F);
            let lower_right = matches!(code, 0x2597 | 0x2599 | 0x259A | 0x259C | 0x259F);
            if upper_left {
                glyph.rects.push([0., 0., x_mid, y_mid]);
            }
            if upper_right {
                glyph.rects.push([x_mid, 0., w - x_mid, y_mid]);
            }
            if lower_left {
                glyph.rects.push([0., y_mid, x_mid, h - y_mid]);
            }
            if lower_right {
                glyph.rects.push([x_mid, y_mid, w - x_mid, h - y_mid]);
            }
        }
    }
    glyph
}

/// Powerline triangles and arrows U+E0B0–U+E0B3. Returns `None` when the cell
/// is too narrow for a triangle, matching Alacritty's builtin font.
fn powerline(code: u32, w: f32, h: f32, t: f32) -> Option<Glyph> {
    let tip = ((h - 1.) / 2.).floor();
    if tip > w + 1. {
        return None;
    }
    let tip = tip.min(w);
    let mirror = matches!(code, 0xE0B2 | 0xE0B3);
    let fx = |x: f32| if mirror { w - x } else { x };
    let top = 1.;
    let bottom = h - 1.;
    let middle = h / 2.;
    let thick = (t - 1.).max(1.);
    let polygon = if matches!(code, 0xE0B0 | 0xE0B2) {
        vec![[fx(0.), top], [fx(tip), middle], [fx(0.), bottom]]
    } else {
        // Thick chevron: outer triangle with an inner triangle removed.
        vec![
            [fx(0.), top],
            [fx(tip), middle],
            [fx(0.), bottom],
            [fx(0.), bottom - thick],
            [fx((tip - thick).max(0.)), middle],
            [fx(0.), top + thick],
        ]
    };
    Some(Glyph {
        polygons: vec![polygon],
        ..Glyph::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::px;

    fn draw(ch: char) -> Glyph {
        glyph(ch, px(9.), px(20.), 1.).unwrap_or_else(|| panic!("{ch} should draw"))
    }

    #[test]
    fn unsupported_characters_fall_back_to_the_font() {
        for ch in ['a', ' ', '日', '\u{2801}', '\u{1FB00}', '\u{E0B4}'] {
            assert!(glyph(ch, px(9.), px(20.), 1.).is_none(), "{ch} drew");
        }
    }

    #[test]
    fn every_supported_character_draws_something_inside_the_cell() {
        for code in 0x2500u32..=0x259F {
            let ch = char::from_u32(code).unwrap();
            let glyph = draw(ch);
            assert!(!glyph.is_empty(), "{ch} drew nothing");
            for [x, y, width, height] in
                glyph.rects.iter().chain(glyph.tints.iter().map(|(r, _)| r))
            {
                assert!(width > &0. && height > &0., "{ch} empty rect");
                assert!(x >= &-1. && y >= &-1., "{ch} rect origin {x},{y}");
                assert!(
                    x + width <= 9. + 1. && y + height <= 20. + 1.,
                    "{ch} rect overflows"
                );
            }
            for stroke in &glyph.strokes {
                assert!(
                    stroke.width > 0. && stroke.points.len() >= 2,
                    "{ch} bad stroke"
                );
            }
            for polygon in &glyph.polygons {
                assert!(polygon.len() >= 3, "{ch} bad polygon");
            }
        }
        for code in [0xE0B0u32, 0xE0B1, 0xE0B2, 0xE0B3] {
            assert!(!draw(char::from_u32(code).unwrap()).is_empty());
        }
    }

    #[test]
    fn lines_span_their_axis_and_blocks_fill_expected_fractions() {
        assert_eq!(draw('─').rects, vec![[0., 10., 9., 1.]]);
        assert_eq!(draw('━').rects, vec![[0., 9., 9., 2.]]);
        assert_eq!(draw('│').rects, vec![[4., 0., 1., 20.]]);
        assert_eq!(draw('█').rects, vec![[0., 0., 9., 20.]]);
        assert_eq!(draw('▀').rects, vec![[0., 0., 9., 10.]]);
        assert_eq!(draw('▁').rects, vec![[0., 17.5, 9., 2.5]]);
        assert_eq!(draw('▐').rects, vec![[4.5, 0., 4.5, 20.]]);
    }

    #[test]
    fn junctions_meet_their_arms() {
        // ├: light vertical stroke and right arm sharing the center row.
        let g = draw('├');
        assert!(g.rects.iter().any(|r| r[0] == 4. && r[3] == 20.));
        assert!(g
            .rects
            .iter()
            .any(|r| r[0] + r[2] == 9. && r[1] <= 10. && r[1] + r[3] >= 10.));
        // ╋: all heavy arms, merged into crossing strokes.
        let g = draw('╋');
        assert!(g.rects.iter().any(|r| r[2] == 2. && r[3] == 20.));
        assert!(g.rects.iter().any(|r| r[2] == 9. && r[3] == 2.));
    }

    #[test]
    fn shades_tint_instead_of_covering() {
        let g = draw('▒');
        assert!(g.rects.is_empty());
        assert_eq!(g.tints.len(), 1);
        assert_eq!(g.tints[0].1, 0x80);
    }

    #[test]
    fn arcs_bend_with_a_stroke_and_a_straight_segment() {
        let g = draw('╭');
        assert_eq!(g.rects.len(), 1);
        assert_eq!(g.strokes.len(), 1);
        assert_eq!(g.strokes[0].points.len(), 4);
        // ╯ exits through the left edge at mid height.
        let g = draw('╯');
        let [x, y] = g.strokes[0].points[3];
        assert_eq!((x, y), (0., 10.));
    }

    #[test]
    fn scale_changes_geometry_in_logical_pixels() {
        // An 18x40 physical cell with a 2px stroke is 9x1 in logical pixels.
        let doubled = glyph('─', px(9.), px(20.), 2.).unwrap();
        assert_eq!(doubled.rects[0], [0., 9.5, 9., 1.]);
    }

    #[test]
    fn narrow_cells_fall_back_on_powerline_triangles() {
        assert!(glyph('\u{E0B0}', px(1.), px(100.), 1.).is_none());
        assert_eq!(draw('\u{E0B0}').polygons.len(), 1);
        let g = draw('\u{E0B3}');
        assert!(g.polygons[0].iter().all(|[x, _]| *x <= 9.));
    }
}
