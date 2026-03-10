use std::collections::HashMap;
use std::path::Path;

use cosmic::iced::Color;

struct AlbumColorBucket {
    count: u32,
    sum_r: u64,
    sum_g: u64,
    sum_b: u64,
    sum_s: f32,
    sum_l: f32,
}

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
    let r = (color.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (color.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (color.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (hue, saturation, lightness) = rgb_to_hsl(r, g, b);

    hsl_to_rgb(hue, saturation.clamp(0.40, 0.72), lightness.clamp(0.38, 0.62))
}

fn album_bucket_score(bucket: &AlbumColorBucket) -> f32 {
    let count = bucket.count as f32;
    let avg_s = bucket.sum_s / count;
    let avg_l = bucket.sum_l / count;
    let lightness_bias = (1.0 - (avg_l - 0.55).abs()).max(0.2);

    count * (0.35 + avg_s) * lightness_bias
}

pub fn dominant_album_color(path: Option<&Path>) -> Option<Color> {
    let path = path?;
    let image = image::open(path).ok()?.to_rgba8();
    let thumb = image::imageops::thumbnail(&image, 48, 48);

    let mut buckets: HashMap<(u16, u8, u8), AlbumColorBucket> = HashMap::new();
    for pixel in thumb.pixels() {
        let [r, g, b, a] = pixel.0;
        if a < 24 {
            continue;
        }

        let (hue, saturation, lightness) = rgb_to_hsl(r, g, b);
        if saturation < 0.18 || !(0.12..=0.82).contains(&lightness) {
            continue;
        }

        // Quantize by hue first so vivid families win over large neutral backgrounds.
        let hue_bin = ((hue / 20.0).floor() as u16) % 18;
        let sat_bin = (saturation * 4.0).floor() as u8;
        let light_bin = (lightness * 4.0).floor() as u8;
        let entry = buckets
            .entry((hue_bin, sat_bin, light_bin))
            .or_insert(AlbumColorBucket {
                count: 0,
                sum_r: 0,
                sum_g: 0,
                sum_b: 0,
                sum_s: 0.0,
                sum_l: 0.0,
            });
        entry.count += 1;
        entry.sum_r += u64::from(r);
        entry.sum_g += u64::from(g);
        entry.sum_b += u64::from(b);
        entry.sum_s += saturation;
        entry.sum_l += lightness;
    }

    let (_, bucket) = buckets
        .into_iter()
        .max_by(|(_, left), (_, right)| album_bucket_score(left).total_cmp(&album_bucket_score(right)))?;
    if bucket.count == 0 {
        return None;
    }

    let r = (bucket.sum_r / u64::from(bucket.count)) as u8;
    let g = (bucket.sum_g / u64::from(bucket.count)) as u8;
    let b = (bucket.sum_b / u64::from(bucket.count)) as u8;
    Some(normalize_album_color(Color::from_rgb8(r, g, b)))
}