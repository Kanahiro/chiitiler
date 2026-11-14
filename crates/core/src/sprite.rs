//! Sprites (style `sprite`): JSON index + sheet image, `@2x` for pixel ratios above 1.
//! Images of sprite `id` are registered as `id:name`, except for the `default` sprite.

use crate::image::{Image, Texture};
use serde_json::Value as J;
use crate::hash::HashMap;
use std::rc::Rc;

pub struct StyleImage {
    pub image: Image,
    pub pixel_ratio: f64,
    pub sdf: bool,
    /// The image with 1px wrapped padding, as packed for patterns.
    pub pattern: Texture,
    /// The image with 1px transparent padding, as packed for icons.
    pub icon: Texture,
    pub stretch_x: Option<Vec<[f64; 2]>>,
    pub stretch_y: Option<Vec<[f64; 2]>>,
    /// `[x1, y1, x2, y2]` in image pixels.
    pub content: Option<[f64; 4]>,
    pub text_fit_width: Option<String>,
    pub text_fit_height: Option<String>,
}

struct Source {
    id: String,
    json_url: String,
    png_url: String,
    json: Option<Option<J>>,
    png: Option<Option<Image>>,
}

#[derive(Default)]
pub struct Sprites {
    sources: Vec<Source>,
    pub images: HashMap<String, Rc<StyleImage>>,
}

/// `normalizeSpriteURL`: append `@2x` and the extension to the path, before any query.
fn sprite_url(url: &str, format: &str, ext: &str) -> String {
    let split = url.find(['?', '#']).unwrap_or(url.len());
    format!("{}{format}{ext}{}", &url[..split], &url[split..])
}

impl Sprites {
    pub fn new(sprite: &J, pixel_ratio: f64) -> Sprites {
        let format = if pixel_ratio > 1.0 { "@2x" } else { "" };
        let list: Vec<(String, String)> = match sprite {
            J::String(url) => vec![("default".into(), url.clone())],
            J::Array(a) => a.iter().filter_map(|s| Some((s["id"].as_str()?.to_string(), s["url"].as_str()?.to_string()))).collect(),
            _ => vec![],
        };
        Sprites {
            sources: list
                .into_iter()
                .map(|(id, url)| Source {
                    id,
                    json_url: sprite_url(&url, format, ".json"),
                    png_url: sprite_url(&url, format, ".png"),
                    json: None,
                    png: None,
                })
                .collect(),
            images: HashMap::default(),
        }
    }

    pub fn needs(&self) -> Vec<String> {
        let mut out = Vec::new();
        for s in &self.sources {
            if s.json.is_none() {
                out.push(s.json_url.clone());
            }
            if s.png.is_none() {
                out.push(s.png_url.clone());
            }
        }
        out
    }

    pub fn provide(&mut self, url: &str, bytes: Option<&[u8]>) {
        let mut changed = false;
        for s in &mut self.sources {
            if s.json.is_none() && s.json_url == url {
                s.json = Some(bytes.and_then(|b| serde_json::from_slice(b).ok()));
                changed = true;
            }
            if s.png.is_none() && s.png_url == url {
                s.png = Some(bytes.and_then(Image::decode));
                changed = true;
            }
        }
        if !changed {
            return;
        }
        for s in &self.sources {
            let (Some(Some(json)), Some(Some(sheet))) = (&s.json, &s.png) else { continue };
            for (name, e) in json.as_object().into_iter().flatten() {
                let key = if s.id == "default" { name.clone() } else { format!("{}:{name}", s.id) };
                if self.images.contains_key(&key) {
                    continue;
                }
                let n = |k: &str| e[k].as_u64().unwrap_or(0) as usize;
                let image = sheet.sub_image(n("x"), n("y"), n("width"), n("height"));
                if image.w == 0 || image.h == 0 {
                    continue;
                }
                let pattern = Texture::new(&image.with_wrapped_padding(), false);
                let icon = Texture::new(&image.with_padding(), false);
                let ranges = |k: &str| e[k].as_array().map(|a| a.iter().map(|r| [r[0].as_f64().unwrap_or(0.0), r[1].as_f64().unwrap_or(0.0)]).collect());
                let content = e["content"].as_array().filter(|c| c.len() == 4).map(|c| std::array::from_fn(|i| c[i].as_f64().unwrap_or(0.0)));
                self.images.insert(
                    key,
                    Rc::new(StyleImage {
                        pixel_ratio: e["pixelRatio"].as_f64().unwrap_or(1.0),
                        sdf: e["sdf"].as_bool().unwrap_or(false),
                        image,
                        pattern,
                        icon,
                        stretch_x: ranges("stretchX"),
                        stretch_y: ranges("stretchY"),
                        content,
                        text_fit_width: e["textFitWidth"].as_str().map(String::from),
                        text_fit_height: e["textFitHeight"].as_str().map(String::from),
                    }),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert_eq!(sprite_url("http://a/s", "@2x", ".json"), "http://a/s@2x.json");
        assert_eq!(sprite_url("http://a/s?k=1", "", ".png"), "http://a/s.png?k=1");
    }
}
