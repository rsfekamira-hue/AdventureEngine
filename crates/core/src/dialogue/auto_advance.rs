//! AUTOの待ち時間。進行できるかの判断は親から受け取る。
/// 待ち時間と有効状態だけを保持し、選択肢やEventを参照しない。
#[derive(Default)]
pub struct AutoAdvance {
    enabled: bool,
    elapsed: f64,
}
impl AutoAdvance {
    /// AUTOを切り替え、途中の待ち時間を破棄する。
    pub fn toggle(&mut self) {
        self.enabled = !self.enabled;
        self.cancel_wait();
    }
    /// 入力や本文更新で、旧本文の待ち時間を持ち越さない。
    pub fn cancel_wait(&mut self) {
        self.elapsed = 0.0;
    }
    /// 再開始と結末でAUTOを無効にする。
    pub fn reset(&mut self) {
        self.enabled = false;
        self.cancel_wait();
    }
    /// 親が渡す可否が成立している間だけ待ち、通常の送り要求を返す。
    pub fn tick(&mut self, seconds: f64, allowed: bool, delay: f64) -> bool {
        if !self.enabled || !allowed {
            self.cancel_wait();
            return false;
        }
        self.elapsed += seconds;
        if self.elapsed >= delay {
            self.cancel_wait();
            true
        } else {
            false
        }
    }
    /// 設定状態を表示情報へ渡す。
    pub fn enabled(&self) -> bool {
        self.enabled
    }
}
