//! Where the time of one edit goes, with no window. Ignored by default: run with
//! `cargo test --release --test cost -- --ignored --nocapture`.
//!
//! For each size it makes 20 edits in the middle line and times, per edit:
//! the C tokenizer and the layout job over the whole text, egui's layout of the
//! whole job, and the same two steps over 60 lines only, which is what a view
//! that draws visible lines only would do for a full screen.

use eframe::egui;
use egui::text::{LayoutJob, TextFormat};
use egui_kittest::Harness;
use k7_editor::{c_lex, sample_c};
use std::time::Instant;

fn job(text: &str, font: &egui::FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (r, _) in c_lex::spans(text) {
        job.append(
            &text[r],
            0.0,
            TextFormat::simple(font.clone(), egui::Color32::WHITE),
        );
    }
    job
}

#[test]
#[ignore]
fn cost_of_one_edit() {
    let done = std::sync::atomic::AtomicBool::new(false);
    let mut harness = Harness::new_ui(|ui| {
        if done.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let font = egui::TextStyle::Monospace.resolve(ui.style());
        for n in [500usize, 5000, 20000] {
            let mut text = sample_c(n);
            let mid = text.match_indices('\n').nth(n / 2).unwrap().0;
            // Warm the paragraph cache.
            let g = ui.fonts_mut(|f| f.layout_job(job(&text, &font)));
            assert_eq!(g.rows.len(), n + 1);
            let (mut tok, mut lay, mut vis_tok, mut vis_lay) = (0.0, 0.0, 0.0, 0.0);
            let edits = 20;
            for _ in 0..edits {
                text.insert(mid, 'x');
                let t = Instant::now();
                let j = job(&text, &font);
                tok += t.elapsed().as_secs_f64();
                let t = Instant::now();
                let g = ui.fonts_mut(|f| f.layout_job(j));
                lay += t.elapsed().as_secs_f64();
                assert_eq!(g.rows.len(), n + 1);

                // 60 lines around the edit, as a visible-lines view would lay out.
                let start = text[..mid].match_indices('\n').rev().nth(30).unwrap().0 + 1;
                let end = text[mid..].match_indices('\n').nth(30).unwrap().0 + mid;
                let t = Instant::now();
                let j = job(&text[start..end], &font);
                vis_tok += t.elapsed().as_secs_f64();
                let t = Instant::now();
                let g = ui.fonts_mut(|f| f.layout_job(j));
                vis_lay += t.elapsed().as_secs_f64();
                assert_eq!(g.rows.len(), 61);
            }
            let ms = |s: f64| s * 1000.0 / edits as f64;
            println!(
                "k7 cost lines={n} whole_tokenize_ms={:.3} whole_layout_ms={:.3} visible60_tokenize_ms={:.3} visible60_layout_ms={:.3}",
                ms(tok),
                ms(lay),
                ms(vis_tok),
                ms(vis_lay)
            );
        }
    });
    harness.run();
}
