//! 進行を止める要求IDの正本。演出の時間は保持しない。
/// 演出要求が未完了かを型で表す。GameInfoはここから読み取り値を作る。
#[derive(Clone, Default)]
pub(super) enum ProgressGate {
    #[default]
    Open,
    AwaitingPresentation {
        request_id: u64,
    },
}
impl ProgressGate {
    /// 読み取り情報へ現在の完了要求を渡す。
    pub(super) fn request(&self) -> Option<u64> {
        match self {
            Self::Open => None,
            Self::AwaitingPresentation { request_id } => Some(*request_id),
        }
    }
    /// 一致する完了通知だけで待機を解除する。
    pub(super) fn finish(&mut self, id: u64) -> bool {
        if self.request() != Some(id) {
            return false;
        }
        *self = Self::Open;
        true
    }
}
