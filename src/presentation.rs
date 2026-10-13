//! Pure helpers for presenting clips (menu labels, timestamps, icon).

use chrono::{DateTime, Utc};

use crate::model::Clip;

/// Label for a clip in the menu-bar menu, e.g. `3. Some text…`.
pub fn menu_label(index: usize, clip: &Clip, max_chars: usize) -> String {
    let title = clip.display_title(max_chars);
    let title = if title.is_empty() {
        "(empty)".to_string()
    } else {
        title
    };
    if index < 9 {
        format!("{}. {title}", index + 1)
    } else {
        title
    }
}

/// Short relative time such as `just now`, `5 min ago`, `3 h ago`, `2 d ago`.
pub fn relative_time(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - at).num_seconds().max(0);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        86_400..=2_591_999 => format!("{} d ago", secs / 86_400),
        _ => at.format("%Y-%m-%d").to_string(),
    }
}

/// Secondary line for a clip: source app, age and paste count.
pub fn clip_subtitle(clip: &Clip, now: DateTime<Utc>) -> String {
    let mut parts = Vec::new();
    if let Some(app) = &clip.source_app {
        parts.push(app.clone());
    }
    parts.push(relative_time(clip.recorded_at, now));
    if clip.paste_count > 0 {
        parts.push(format!("pasted {}×", clip.paste_count));
    }
    if clip.rtf.is_some() || clip.html.is_some() {
        parts.push("rich text".into());
    }
    parts.join(" · ")
}

/// One piece of the paperclip wire, in "clip space" (vertical clip,
/// y pointing up, roughly 0.6 wide and 1.6 tall, centred on the origin).
#[derive(Debug, Clone, Copy)]
enum Wire {
    /// Straight segment between two points.
    Line([f32; 2], [f32; 2]),
    /// Half circle: centre, radius, `true` = upper half, `false` = lower half.
    Arc([f32; 2], f32, bool),
}

/// The classic paperclip: three nested U-shapes made of one wire.
const PAPERCLIP: [Wire; 7] = [
    Wire::Line([0.15, -0.25], [0.15, 0.35]),
    Wire::Arc([0.0, 0.35], 0.15, true),
    Wire::Line([-0.15, 0.35], [-0.15, -0.55]),
    Wire::Arc([0.075, -0.55], 0.225, false),
    Wire::Line([0.3, -0.55], [0.3, 0.55]),
    Wire::Arc([0.0, 0.55], 0.3, true),
    Wire::Line([-0.3, 0.55], [-0.3, -0.35]),
];

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

fn wire_distance(w: Wire, p: [f32; 2]) -> f32 {
    match w {
        Wire::Line(a, b) => {
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let len2 = dx * dx + dy * dy;
            let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0);
            dist(p, [a[0] + t * dx, a[1] + t * dy])
        }
        Wire::Arc(c, r, upper) => {
            let on_side = if upper { p[1] >= c[1] } else { p[1] <= c[1] };
            if on_side {
                (dist(p, c) - r).abs()
            } else {
                dist(p, [c[0] - r, c[1]]).min(dist(p, [c[0] + r, c[1]]))
            }
        }
    }
}

/// Distance from a clip-space point to the paperclip wire.
fn paperclip_distance(p: [f32; 2]) -> f32 {
    PAPERCLIP
        .iter()
        .map(|w| wire_distance(*w, p))
        .fold(f32::INFINITY, f32::min)
}

/// Tilt of the paperclip in the menu-bar icon (radians, clockwise).
const CLIP_TILT: f32 = std::f32::consts::FRAC_PI_4;
/// Vertical centre of the clip in clip space.
const CLIP_CENTER_Y: f32 = 0.0375;
/// Half the diagonal extent of the tilted clip in clip units.
const CLIP_HALF_EXTENT: f32 = 0.8;

/// Map an icon pixel centre to clip space.
fn pixel_to_clip(x: f32, y: f32, size: f32) -> [f32; 2] {
    let scale = (size / 2.0 - size * 0.05) / CLIP_HALF_EXTENT;
    // Pixel space has y pointing down; clip space has y pointing up.
    let (px, py) = ((x - size / 2.0) / scale, (size / 2.0 - y) / scale);
    // Rotate counter-clockwise by the tilt to undo the clockwise tilt.
    let (sin, cos) = CLIP_TILT.sin_cos();
    [px * cos - py * sin, px * sin + py * cos + CLIP_CENTER_Y]
}

/// RGBA pixels for the template menu-bar icon (a single tilted paperclip),
/// `size`×`size`. Only the alpha channel matters for template images, so
/// macOS tints the icon for light and dark menu bars automatically.
pub fn tray_icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let scale = (s / 2.0 - s * 0.05) / CLIP_HALF_EXTENT;
    // Wire thickness in pixels (≈1.5 pt at 44 px).
    let half_width = (s * 0.034).max(0.75);
    let mut px = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let p = pixel_to_clip(x as f32 + 0.5, y as f32 + 0.5, s);
            let d = paperclip_distance(p) * scale;
            // Anti-aliased edge: full coverage inside, linear ramp over 1 px.
            let coverage = (half_width + 0.5 - d).clamp(0.0, 1.0);
            if coverage > 0.0 {
                let i = ((y * size + x) * 4) as usize;
                px[i + 3] = (coverage * 255.0).round() as u8;
            }
        }
    }
    px
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn clip(contents: &str) -> Clip {
        Clip {
            id: 1,
            contents: contents.into(),
            rtf: None,
            html: None,
            source_app: Some("Mail".into()),
            source_bundle: None,
            recorded_at: Utc::now(),
            pinned: false,
            pin_order: 0,
            title: None,
            paste_count: 0,
        }
    }

    #[test]
    fn menu_label_numbers_first_nine() {
        assert_eq!(menu_label(0, &clip("hello"), 50), "1. hello");
        assert_eq!(menu_label(8, &clip("x"), 50), "9. x");
        assert_eq!(menu_label(9, &clip("x"), 50), "x");
    }

    #[test]
    fn menu_label_for_blank_clip() {
        assert_eq!(menu_label(0, &clip("   "), 50), "1. (empty)");
    }

    #[test]
    fn relative_time_buckets() {
        let now = Utc::now();
        assert_eq!(relative_time(now, now), "just now");
        assert_eq!(relative_time(now - Duration::minutes(5), now), "5 min ago");
        assert_eq!(relative_time(now - Duration::hours(3), now), "3 h ago");
        assert_eq!(relative_time(now - Duration::days(2), now), "2 d ago");
        let old = now - Duration::days(100);
        assert_eq!(relative_time(old, now), old.format("%Y-%m-%d").to_string());
        assert_eq!(relative_time(now + Duration::minutes(1), now), "just now");
    }

    #[test]
    fn subtitle_lists_source_age_and_pastes() {
        let mut c = clip("x");
        c.paste_count = 3;
        c.rtf = Some(vec![1]);
        assert_eq!(
            clip_subtitle(&c, c.recorded_at),
            "Mail · just now · pasted 3× · rich text"
        );
    }

    #[test]
    fn tray_icon_has_right_size_and_some_opaque_pixels() {
        let px = tray_icon_rgba(44);
        assert_eq!(px.len(), 44 * 44 * 4);
        let opaque = px.chunks(4).filter(|p| p[3] > 128).count();
        assert!(opaque > 100 && opaque < 44 * 44 / 4, "opaque = {opaque}");
        // Template image: colour channels stay zero, only alpha is used.
        assert!(px.chunks(4).all(|p| p[0] == 0 && p[1] == 0 && p[2] == 0));
        // Corners are transparent.
        assert_eq!(px[3], 0);
        assert_eq!(px[px.len() - 1], 0);
    }

    #[test]
    fn tray_icon_loops_are_open() {
        let size = 44u32;
        let px = tray_icon_rgba(size);
        // Centres of the inner and middle loops lie between wires.
        for c in [[0.0, 0.35], [0.075, -0.55], [0.0, 0.0]] {
            let (x, y) = clip_to_pixel(c, size as f32);
            let i = ((y * size + x) * 4) as usize;
            assert_eq!(px[i + 3], 0, "loop centre {c:?} should be transparent");
        }
    }

    #[test]
    fn tray_icon_wire_is_drawn() {
        let size = 44u32;
        let px = tray_icon_rgba(size);
        // Points on the wire are opaque.
        for c in [[0.15, 0.0], [-0.15, -0.2], [0.3, 0.2], [0.0, 0.85]] {
            let (x, y) = clip_to_pixel(c, size as f32);
            let i = ((y * size + x) * 4) as usize;
            assert!(px[i + 3] > 128, "wire point {c:?} should be opaque");
        }
    }

    #[test]
    fn wire_distance_basics() {
        assert!((wire_distance(Wire::Line([0.0, 0.0], [0.0, 1.0]), [0.5, 0.5]) - 0.5).abs() < 1e-6);
        assert!((wire_distance(Wire::Arc([0.0, 0.0], 1.0, true), [0.0, 2.0]) - 1.0).abs() < 1e-6);
        // Below an upper arc: distance to the nearest endpoint.
        assert!((wire_distance(Wire::Arc([0.0, 0.0], 1.0, true), [1.0, -1.0]) - 1.0).abs() < 1e-6);
    }

    /// Inverse of `pixel_to_clip`, rounded to the nearest pixel.
    fn clip_to_pixel(c: [f32; 2], size: f32) -> (u32, u32) {
        let scale = (size / 2.0 - size * 0.05) / CLIP_HALF_EXTENT;
        let (sin, cos) = CLIP_TILT.sin_cos();
        let (cx, cy) = (c[0], c[1] - CLIP_CENTER_Y);
        let px = cx * cos + cy * sin;
        let py = -cx * sin + cy * cos;
        let x = px * scale + size / 2.0;
        let y = size / 2.0 - py * scale;
        let (x, y) = (x.floor() as u32, y.floor() as u32);
        let back = pixel_to_clip(x as f32 + 0.5, y as f32 + 0.5, size);
        assert!(dist(back, c) < 1.5 / scale, "roundtrip {c:?} -> {back:?}");
        (x, y)
    }
}
