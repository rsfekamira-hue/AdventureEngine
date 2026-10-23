//! 文字送りの正本。書記素境界と表示速度だけを管理する。
/// 文字列をコピーせず、書記素境界で分けた表示済み・未表示部分を渡す。
pub struct TextDisplay<'a> {
    pub visible: &'a str,
    pub hidden: &'a str,
    pub typing: bool,
    pub speed_label: &'static str,
    pub glyph_count: usize,
}

use unicode_segmentation::UnicodeSegmentation;
/// 空配列・不正な速度・範囲外の選択を許さず、速度の切替規則を所有する。
#[derive(Clone)]
pub struct TextSpeeds {
    rates: Vec<f64>,
    selected: usize,
}
impl TextSpeeds {
    pub fn new(rates: Vec<f64>, selected: usize) -> Result<Self, String> {
        if rates.is_empty()
            || selected >= rates.len()
            || rates.iter().any(|rate| !rate.is_finite() || *rate < 0.0)
        {
            return Err("文字速度の設定が不正です".into());
        }
        Ok(Self { rates, selected })
    }
    fn current(&self) -> f64 {
        self.rates[self.selected]
    }
    fn cycle(&mut self) {
        self.selected = (self.selected + 1) % self.rates.len();
    }
    fn label(&self) -> &'static str {
        match self.selected {
            0 => "ゆっくり",
            1 => "標準",
            2 => "速い",
            _ => "瞬時",
        }
    }
}
/// 本文と書記素の末尾位置を所有し、毎フレーム文字列を作り直さない。
pub struct TextPlayback {
    text: String,
    ends: Vec<usize>,
    position: f64,
    speeds: TextSpeeds,
}
impl TextPlayback {
    /// 検証済みの速度設定を保持する。設定変更をゲーム進行へ混ぜない。
    pub fn new(speeds: TextSpeeds) -> Self {
        Self {
            text: String::new(),
            ends: vec![],
            position: 0.0,
            speeds,
        }
    }
    /// 新しい本文を一度だけ分割する。メニュー本文は即時表示する。
    pub fn start(&mut self, text: &str, instant: bool) {
        self.text = text.into();
        self.ends = text
            .grapheme_indices(true)
            .map(|(i, g)| i + g.len())
            .collect();
        self.position = if instant { self.ends.len() as f64 } else { 0.0 };
    }
    /// 正規化済みの時間で表示位置を進める。0速度は瞬時表示を意味する。
    pub fn tick(&mut self, seconds: f64) {
        let speed = self.speeds.current();
        self.position = if speed == 0.0 {
            self.ends.len() as f64
        } else {
            (self.position + seconds * speed).min(self.ends.len() as f64)
        };
    }
    /// 本文を表示し切る。進行操作は返さない。
    pub fn complete(&mut self) {
        self.position = self.ends.len() as f64;
    }
    /// 次の表示速度へ切り替える。
    pub fn change_speed(&mut self) {
        self.speeds.cycle();
    }
    /// 同じ全文から前後を借用し、改行計測と描画が同じ文字列を使うようにする。
    pub fn info(&self) -> TextDisplay<'_> {
        let count = self.position as usize;
        let end = if count == 0 { 0 } else { self.ends[count - 1] };
        TextDisplay {
            visible: &self.text[..end],
            hidden: &self.text[end..],
            typing: count < self.ends.len(),
            speed_label: self.speeds.label(),
            glyph_count: self.ends.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speed_settings_reject_invalid_rates_and_selection() {
        for rates in [vec![], vec![-1.0], vec![f64::NAN], vec![f64::INFINITY]] {
            assert!(TextSpeeds::new(rates, 0).is_err());
        }
        assert!(TextSpeeds::new(vec![1.0], 1).is_err());
    }

    #[test]
    fn cycling_preserves_grapheme_boundaries_and_instant_display() {
        let mut text = TextPlayback::new(TextSpeeds::new(vec![1.0, 0.0], 0).unwrap());
        text.start("か\u{3099}👩‍💻終", false);
        text.tick(1.0);
        assert_eq!(text.info().visible, "か\u{3099}");
        assert_eq!(text.info().hidden, "👩‍💻終");
        assert!(text.info().typing);
        text.change_speed();
        text.tick(0.0);
        assert_eq!(text.info().visible, "か\u{3099}👩‍💻終");
        assert!(!text.info().typing);
        text.change_speed();
        text.start("次", false);
        text.tick(0.5);
        assert_eq!(text.info().visible, "");
        text.tick(0.5);
        assert_eq!(text.info().visible, "次");
    }
}
