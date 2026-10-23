//! 文字・四角形の汎用パーティクル。時刻とseedから再現でき、GPUや人物に依存しない。
use serde::Deserialize;

#[derive(Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ParticleShape {
    Text {
        characters: String,
        change_ms: f64,
    },
    Phrases {
        phrases: Vec<String>,
        change_ms: f64,
    },
    Quad,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParticleEmitter {
    pub seed: u32,
    pub count: u16,
    pub lifetime_ms: f64,
    pub shape: ParticleShape,
    /// 発生矩形と移動はエフェクト領域に対する比率。速度は毎秒。
    pub spawn: [f32; 4],
    pub velocity: [f32; 2],
    pub spread: [f32; 2],
    pub acceleration: [f32; 2],
    /// 指定すると寿命の後半にこの矩形内へ集まる。
    pub destination: Option<[f32; 4]>,
    /// 対象の高さに対する開始・終了サイズ。
    pub size: [f32; 2],
    pub color: [f32; 4],
    pub glow: f32,
    #[serde(default)]
    pub glitch: f32,
    pub trails: u8,
    pub trail_ms: f64,
}

pub struct Particle {
    pub position: [f32; 2],
    pub text: Option<String>,
    pub glitch: f32,
    pub size: f32,
    pub color: [f32; 4],
    pub glow: f32,
}

fn hash(mut x: u32) -> u32 {
    x = (x ^ (x >> 16)).wrapping_mul(0x7feb352d);
    x = (x ^ (x >> 15)).wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}
fn random(seed: u32, lane: u32) -> f32 {
    hash(seed.wrapping_add(lane.wrapping_mul(0x9e3779b9))) as f32 / u32::MAX as f32
}

impl ParticleEmitter {
    pub fn validate(&self) -> Result<(), String> {
        let valid_rect = |r: &[f32; 4]| {
            r.iter().all(|v| v.is_finite() && (-2.0..=3.0).contains(v))
                && r[2] >= 0.0
                && r[3] >= 0.0
        };
        if self.count == 0
            || self.count > 384
            || !self.lifetime_ms.is_finite()
            || !(100.0..=60000.0).contains(&self.lifetime_ms)
            || !valid_rect(&self.spawn)
            || self.destination.as_ref().is_some_and(|r| !valid_rect(r))
            || self
                .velocity
                .iter()
                .chain(&self.spread)
                .chain(&self.acceleration)
                .any(|v| !v.is_finite() || v.abs() > 10.0)
            || self.spread.iter().any(|v| *v < 0.0)
            || self
                .size
                .iter()
                .any(|v| !v.is_finite() || !(0.001..=0.2).contains(v))
            || self
                .color
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || !self.glow.is_finite()
            || !(0.0..=1.0).contains(&self.glow)
            || !self.glitch.is_finite()
            || !(0.0..=1.0).contains(&self.glitch)
            || self.trails > 8
            || !self.trail_ms.is_finite()
            || !(1.0..=1000.0).contains(&self.trail_ms)
        {
            return Err("パーティクルの数・寿命・位置・速度・色・残像指定が不正です".into());
        }
        if let ParticleShape::Text {
            characters,
            change_ms,
        } = &self.shape
        {
            if characters.is_empty()
                || characters.chars().count() > 128
                || characters.chars().any(char::is_control)
                || !change_ms.is_finite()
                || !(30.0..=10000.0).contains(change_ms)
            {
                return Err("パーティクルの文字集合・置換間隔が不正です".into());
            }
        }
        if let ParticleShape::Phrases { phrases, change_ms } = &self.shape {
            if phrases.is_empty()
                || phrases.len() > 32
                || phrases.iter().any(|p| {
                    p.trim().is_empty() || p.chars().count() > 24 || p.chars().any(char::is_control)
                })
                || !change_ms.is_finite()
                || !(100.0..=10000.0).contains(change_ms)
            {
                return Err("パーティクルのフレーズ・置換間隔が不正です".into());
            }
        }
        Ok(())
    }

    /// 世代ごとに軌道を固定。残像は同じ粒子の過去位置から作り、再発生時に線を繋がない。
    pub fn sample(&self, milliseconds: f64, fade: f32, output: &mut Vec<Particle>) {
        let characters: Vec<char> = match &self.shape {
            ParticleShape::Text { characters, .. } => characters.chars().collect(),
            _ => Vec::new(),
        };
        for i in 0..self.count {
            let base = self.seed.wrapping_add(u32::from(i).wrapping_mul(7919));
            let clock = milliseconds.max(0.0) + f64::from(random(base, 0)) * self.lifetime_ms;
            let generation = (clock / self.lifetime_ms).floor() as u32;
            let age = clock % self.lifetime_ms;
            let seed = base.wrapping_add(generation.wrapping_mul(104729));
            for trail in (0..=self.trails).rev() {
                let age = age - f64::from(trail) * self.trail_ms;
                if age < 0.0 {
                    continue;
                }
                let t = (age / self.lifetime_ms) as f32;
                let seconds = (age / 1000.0) as f32;
                let mut position = std::array::from_fn(|axis| {
                    self.spawn[axis]
                        + self.spawn[axis + 2] * random(seed, axis as u32 + 1)
                        + (self.velocity[axis]
                            + self.spread[axis] * (2.0 * random(seed, axis as u32 + 3) - 1.0))
                            * seconds
                        + self.acceleration[axis] * seconds * seconds * 0.5
                });
                if let Some(destination) = self.destination {
                    for axis in 0..2 {
                        let goal = destination[axis]
                            + destination[axis + 2] * random(seed, axis as u32 + 5);
                        position[axis] += (goal - position[axis]) * t * t;
                    }
                }
                let text = match &self.shape {
                    ParticleShape::Text { change_ms, .. } => {
                        let step = (age / change_ms).floor() as u32;
                        Some(
                            characters[hash(seed.wrapping_add(step)) as usize % characters.len()]
                                .to_string(),
                        )
                    }
                    ParticleShape::Phrases { phrases, change_ms } => {
                        let step = (age / change_ms).floor() as u32;
                        Some(
                            phrases[hash(seed.wrapping_add(step)) as usize % phrases.len()].clone(),
                        )
                    }
                    ParticleShape::Quad => None,
                };
                // 短い区間だけ走査位置をずらす。時刻が止まればグリッチも止まる。
                let noise = random(seed, (age / 90.0) as u32 + 20);
                let glitch = if noise > 0.72 {
                    self.glitch * noise
                } else {
                    0.0
                };
                position[0] += glitch * (noise - 0.85) * 0.3;
                let mut color = self.color;
                color[3] *= (t * 12.0).min(1.0)
                    * ((1.0 - t) * 5.0).min(1.0)
                    * 0.68_f32.powi(i32::from(trail))
                    * fade;
                output.push(Particle {
                    position,
                    text,
                    glitch,
                    size: self.size[0] + (self.size[1] - self.size[0]) * t,
                    color,
                    glow: if trail == 0 { self.glow } else { 0.0 },
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn emitter() -> ParticleEmitter {
        serde_json::from_value(serde_json::json!({"seed":1,"count":12,"lifetimeMs":2000,"shape":{"kind":"text","characters":"01AB","changeMs":100},"spawn":[0,0,1,1],"velocity":[0,0.3],"spread":[0.1,0.1],"acceleration":[0,0],"size":[0.03,0.01],"color":[0,1,0.3,1],"glow":0.5,"trails":3,"trailMs":50})).unwrap()
    }
    #[test]
    fn replay_is_deterministic_bounded_and_moves() {
        let e = emitter();
        e.validate().unwrap();
        let (mut a, mut b, mut c) = (Vec::new(), Vec::new(), Vec::new());
        e.sample(850.0, 1.0, &mut a);
        e.sample(850.0, 1.0, &mut b);
        e.sample(950.0, 1.0, &mut c);
        assert!(!a.is_empty() && a.len() <= 48);
        assert_eq!(a[0].position, b[0].position);
        assert_eq!(a[0].text, b[0].text);
        assert_ne!(a[0].position, c[0].position);
        assert!(a.iter().all(|p| p.position.iter().all(|v| v.is_finite())
            && p.color[3] <= 1.0
            && "01AB".contains(p.text.as_deref().unwrap())));
    }
    #[test]
    fn phrases_and_glitches_replay_as_whole_strings() {
        let mut e = emitter();
        let phrases = vec!["OBEY THE MAIDEN".to_string(), "COMMANDS FIRST".to_string()];
        e.shape = ParticleShape::Phrases {
            phrases: phrases.clone(),
            change_ms: 750.0,
        };
        e.glitch = 0.85;
        e.validate().unwrap();
        let (mut a, mut b) = (Vec::new(), Vec::new());
        e.sample(1200.0, 1.0, &mut a);
        e.sample(1200.0, 1.0, &mut b);
        assert!(a.iter().all(|p| phrases.contains(p.text.as_ref().unwrap())));
        assert!(a.iter().any(|p| p.glitch > 0.0));
        for (a, b) in a.iter().zip(&b) {
            assert_eq!(a.text, b.text);
            assert_eq!(a.position, b.position);
            assert_eq!(a.glitch, b.glitch);
        }
        e.shape = ParticleShape::Phrases {
            phrases: vec![],
            change_ms: 750.0,
        };
        assert!(e.validate().is_err());
    }
    #[test]
    fn invalid_text_and_resource_limits_are_rejected() {
        let mut e = emitter();
        e.count = 385;
        assert!(e.validate().is_err());
        e.count = 12;
        e.shape = ParticleShape::Text {
            characters: String::new(),
            change_ms: 100.0,
        };
        assert!(e.validate().is_err());
        e.shape = ParticleShape::Quad;
        e.lifetime_ms = 0.0;
        assert!(e.validate().is_err());
    }
}
