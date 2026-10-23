//! 人物の役割を知らない画像・配置の部品。
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}
impl Layout {
    pub fn valid(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|v| v.is_finite())
            && self.width > 0.0
            && self.height > 0.0
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Portrait {
    #[serde(default)]
    pub base_image: Option<String>,
    #[serde(default)]
    pub expressions: BTreeMap<String, String>,
    #[serde(default)]
    pub layout: Option<Layout>,
}
impl Portrait {
    pub fn image<'a>(
        &'a self,
        expression: &str,
        available: impl Fn(&str) -> bool,
    ) -> Option<&'a str> {
        [
            self.expressions.get(expression),
            self.expressions.get("neutral"),
            self.base_image.as_ref(),
        ]
        .into_iter()
        .flatten()
        .find(|p| available(p))
        .map(String::as_str)
    }
}

/// 表示人物の一覧。一人ずつ役割に依存しない配置・変換を指定する。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActorLayer {
    pub portrait: String,
    #[serde(default = "neutral")]
    pub expression: String,
    pub layout: Layout,
    #[serde(default = "identity")]
    pub transform: [f32; 4],
    #[serde(default = "fully_visible")]
    pub scale_x: f32,
    #[serde(default = "fully_visible")]
    pub reveal: f32,
}
fn neutral() -> String {
    "neutral".into()
}
fn identity() -> [f32; 4] {
    [0.0, 0.0, 1.0, 0.0]
}
fn fully_visible() -> f32 {
    1.0
}
impl ActorLayer {
    pub fn valid(&self) -> bool {
        !self.portrait.is_empty()
            && self.layout.valid()
            && self.transform.iter().all(|v| v.is_finite())
            && self.transform[2] > 0.0
            && self.scale_x.is_finite()
            && self.scale_x != 0.0
            && self.reveal.is_finite()
            && (0.0..=1.0).contains(&self.reveal)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn horizontal_scale_defaults_and_mirrors_without_allowing_zero() {
        let mut value =
            serde_json::json!({"portrait":"p", "layout":{"x":0,"y":100,"width":40,"height":90}});
        let layer: ActorLayer = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(layer.scale_x, 1.0);
        assert!(layer.valid());
        value["scaleX"] = serde_json::json!(-1);
        assert!(serde_json::from_value::<ActorLayer>(value.clone())
            .unwrap()
            .valid());
        value["scaleX"] = serde_json::json!(0);
        assert!(!serde_json::from_value::<ActorLayer>(value).unwrap().valid());
    }
}
