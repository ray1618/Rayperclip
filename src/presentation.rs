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

/// RGBA pixels for a template menu-bar icon (a clipboard), `size`×`size`.
/// Only the alpha channel matters for template images.
pub fn tray_icon_rgba(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    let border = (s / 11.0).max(1.0);
    let (bl, br, bt, bb) = (s * 0.18, s * 0.82, s * 0.16, s * 0.94);
    let (cl, cr, ct, cb) = (s * 0.34, s * 0.66, s * 0.06, s * 0.26);
    for y in 0..size {
        for x in 0..size {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let in_board = fx >= bl && fx <= br && fy >= bt && fy <= bb;
            let board_edge = in_board
                && (fx < bl + border || fx > br - border || fy < bt + border || fy > bb - border);
            let in_clip = fx >= cl && fx <= cr && fy >= ct && fy <= cb;
            let line = in_board
                && fx > bl + 2.0 * border
                && fx < br - 2.0 * border
                && [0.45, 0.6, 0.75]
                    .iter()
                    .any(|r| (fy - s * r).abs() < border / 2.0 + 0.01);
            if board_edge || in_clip || line {
                let i = ((y * size + x) * 4) as usize;
                px[i + 3] = 255;
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
        let px = tray_icon_rgba(32);
        assert_eq!(px.len(), 32 * 32 * 4);
        let opaque = px.chunks(4).filter(|p| p[3] == 255).count();
        assert!(opaque > 50 && opaque < 32 * 32 / 2, "opaque = {opaque}");
        // corners are transparent
        assert_eq!(px[3], 0);
        assert_eq!(px[px.len() - 1], 0);
    }
}
