use std::path::Path;

use cosmic::iced::Color;

#[allow(clippy::float_cmp)]
fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = f32::from(r) / 255.0;
    let g = f32::from(g) / 255.0;
    let b = f32::from(b) / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) * 0.5;
    let delta = max - min;

    if delta <= f32::EPSILON {
        return (0.0, 0.0, lightness);
    }

    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let mut hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        ((b - r) / delta) + 2.0
    } else {
        ((r - g) / delta) + 4.0
    } * 60.0;

    if hue < 0.0 {
        hue += 360.0;
    }

    (hue, saturation.clamp(0.0, 1.0), lightness.clamp(0.0, 1.0))
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> Color {
    let saturation = saturation.clamp(0.0, 1.0);
    let lightness = lightness.clamp(0.0, 1.0);

    if saturation <= f32::EPSILON {
        return Color {
            r: lightness,
            g: lightness,
            b: lightness,
            a: 1.0,
        };
    }

    let hue = hue.rem_euclid(360.0) / 360.0;
    let q = if lightness < 0.5 {
        lightness * (1.0 + saturation)
    } else {
        lightness + saturation - (lightness * saturation)
    };
    let p = 2.0 * lightness - q;

    let hue_to_channel = |mut t: f32| {
        if t < 0.0 {
            t += 1.0;
        }
        if t > 1.0 {
            t -= 1.0;
        }

        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };

    Color {
        r: hue_to_channel(hue + 1.0 / 3.0),
        g: hue_to_channel(hue),
        b: hue_to_channel(hue - 1.0 / 3.0),
        a: 1.0,
    }
}

fn normalize_album_color(color: Color) -> Color {
    let r = channel_to_u8(color.r);
    let g = channel_to_u8(color.g);
    let b = channel_to_u8(color.b);
    let (hue, saturation, lightness) = rgb_to_hsl(r, g, b);

    hsl_to_rgb(
        hue,
        saturation.clamp(0.40, 0.72),
        lightness.clamp(0.38, 0.62),
    )
}

pub fn dominant_album_color(path: &Path) -> Option<Color> {
    let image = image::open(path).ok()?.to_rgba8();
    let thumb = image::imageops::thumbnail(&image, 32, 32);

    let mut sum_r = 0.0f32;
    let mut sum_g = 0.0f32;
    let mut sum_b = 0.0f32;
    let mut total_weight = 0.0f32;

    for pixel in thumb.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 24 {
            continue;
        }

        let (hue, saturation, lightness) = rgb_to_hsl(r, g, b);
        if saturation < 0.18 || !(0.12..=0.82).contains(&lightness) {
            continue;
        }

        let lightness_bias = (1.0 - (lightness - 0.5).abs() * 2.0).max(0.0);
        let hue_bias = if (35.0..=70.0).contains(&hue) {
            0.92
        } else {
            1.0
        };
        let weight = saturation * lightness_bias * hue_bias;
        if weight <= 0.0 {
            continue;
        }

        sum_r += f32::from(r) * weight;
        sum_g += f32::from(g) * weight;
        sum_b += f32::from(b) * weight;
        total_weight += weight;
    }

    // Grey or transparent covers have no colour to use; keep the theme's style.
    if total_weight <= f32::EPSILON {
        return None;
    }

    let r = byte_value_to_u8(sum_r / total_weight);
    let g = byte_value_to_u8(sum_g / total_weight);
    let b = byte_value_to_u8(sum_b / total_weight);
    Some(normalize_album_color(Color::from_rgb8(r, g, b)))
}

fn channel_to_u8(value: f32) -> u8 {
    byte_value_to_u8(value.clamp(0.0, 1.0) * 255.0)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn byte_value_to_u8(value: f32) -> u8 {
    value.clamp(0.0, 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use tempfile::tempdir;

    fn color_from_image(pixel: Rgba<u8>) -> Option<Color> {
        let directory = tempdir().unwrap();
        let path = directory.path().join("cover.png");
        RgbaImage::from_pixel(4, 4, pixel).save(&path).unwrap();
        dominant_album_color(&path)
    }

    #[test]
    fn transparent_and_neutral_images_have_no_album_color() {
        assert_eq!(color_from_image(Rgba([200, 20, 20, 0])), None);
        assert_eq!(color_from_image(Rgba([128, 128, 128, 255])), None);
        assert_eq!(color_from_image(Rgba([0, 0, 0, 255])), None);
        assert_eq!(color_from_image(Rgba([255, 255, 255, 255])), None);
    }

    #[test]
    fn saturated_color_is_preserved_and_normalized() {
        let color = color_from_image(Rgba([220, 30, 30, 255])).unwrap();

        assert!(color.r > color.g);
        assert!(color.r > color.b);
        let (_, saturation, lightness) = rgb_to_hsl(
            channel_to_u8(color.r),
            channel_to_u8(color.g),
            channel_to_u8(color.b),
        );
        assert!((0.40..=0.73).contains(&saturation));
        assert!((0.38..=0.63).contains(&lightness));
    }

    #[test]
    fn dark_and_light_colors_are_bounded_for_readability() {
        for pixel in [Rgba([80, 0, 0, 255]), Rgba([255, 150, 150, 255])] {
            let color = color_from_image(pixel).unwrap();
            let (_, _, lightness) = rgb_to_hsl(
                channel_to_u8(color.r),
                channel_to_u8(color.g),
                channel_to_u8(color.b),
            );
            assert!((0.37..=0.63).contains(&lightness));
        }
    }
}
