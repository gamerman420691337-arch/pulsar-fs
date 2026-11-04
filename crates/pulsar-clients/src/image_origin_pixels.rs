//! Raw retained pixels use the same authenticated local preview ABI as video.
use crate::preview::{self, DecodedFrame};
use pulsar_protocol::CandidateImageOriginPixels;

pub(crate) fn decode(pixels: &CandidateImageOriginPixels) -> anyhow::Result<DecodedFrame> {
    pixels.validate()?;
    preview::decode_frame(&pixels.frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;
    use std::path::PathBuf;

    #[test]
    #[ignore = "requires explicitly captured actual local and clone pixel observations"]
    fn native_local_and_clone_pixels_decode_and_construct_headless_candidate_images(
    ) -> anyhow::Result<()> {
        let directory = PathBuf::from(
            std::env::var_os("PULSAR_IMAGE_PIXEL_OBSERVATIONS")
                .expect("actual observations required"),
        );
        anyhow::ensure!(directory.is_absolute());
        let mut count = 0;
        for entry in std::fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let mut pixels: CandidateImageOriginPixels =
                serde_json::from_slice(&std::fs::read(&path)?)?;
            pixels.validate()?;
            let original_path = pixels.frame.artifact.path.clone();
            // Only the observation location changes. Identity/layout and the
            // complete original candidate/approval bindings stay unchanged.
            pixels.frame.artifact.path = path.with_extension("rgb");
            let frame = decode(&pixels)?;
            let rgb = std::fs::read(&pixels.frame.artifact.path)?;
            assert_eq!(frame.rgba.len(), frame.width * frame.height * 4);
            let (rgba_channels, rgba_remainder) = frame.rgba.as_chunks::<4>();
            let (rgb_channels, rgb_remainder) = rgb.as_chunks::<3>();
            assert!(rgba_remainder.is_empty() && rgb_remainder.is_empty());
            assert_eq!(rgba_channels.len(), rgb_channels.len());
            for (rgba, original) in rgba_channels.iter().zip(rgb_channels.iter()) {
                assert_eq!(&rgba[..3], original);
                assert_eq!(rgba[3], 255);
            }
            let context = egui::Context::default();
            let mut patterns_review = crate::pattern_review::PatternReviews::default();
            let proposal_before = serde_json::to_vec(&pixels.report)?;
            let output = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 1600.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    let texture = ctx.load_texture(
                        "actual-retained-image",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [frame.width, frame.height],
                            &frame.rgba,
                        ),
                        egui::TextureOptions::LINEAR,
                    );
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.image((texture.id(), texture.size_vec2()));
                        for origin in &pixels.report.origins {
                            if let Some(pulsar_protocol::AssistantProposal::SynthesizePatterns {
                                patterns,
                            }) = &origin.approval.report.proposal
                            {
                                patterns_review.show(ui, patterns);
                            }
                        }
                    });
                },
            );
            assert!(!output.shapes.is_empty());
            assert!(!output.textures_delta.set.is_empty());
            assert!(
                output
                    .shapes
                    .iter()
                    .any(|s| matches!(&s.shape, egui::Shape::Path(p) if p.points.len() == 17)),
                "retained original proposal graph missing"
            );
            assert_eq!(serde_json::to_vec(&pixels.report)?, proposal_before);
            eprintln!("historical local/clone retained pattern review: exact 17-sample graph constructed headlessly; original proposal unchanged PASS");

            let original = rgb.clone();
            let mut corrupted = rgb;
            corrupted[0] ^= 1;
            std::fs::write(&pixels.frame.artifact.path, corrupted)?;
            assert!(
                decode(&pixels).is_err(),
                "changed pixels must not reach rendering"
            );
            std::fs::write(&pixels.frame.artifact.path, original)?;
            assert!(decode(&pixels).is_ok());
            eprintln!(
                "actual retained image decoded/rendered: {}x{} {} current-engine-path={}",
                frame.width,
                frame.height,
                pixels.record.sha256,
                original_path.display()
            );
            count += 1;
        }
        assert_eq!(
            count, 5,
            "one actual local and four clone observations required"
        );
        Ok(())
    }
}
