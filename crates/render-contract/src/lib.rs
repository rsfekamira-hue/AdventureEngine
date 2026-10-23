//! ゲームから描画器へ渡すデータ。GPUやブラウザのAPIには依存しない。

/// 単体スプライト描画で画像を識別する番号。ファイル名やGPUハンドルを描画データに持ち込まないために使う。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureId(pub u32); // 読み込んだ画像の識別番号

/// 一枚の画像の論理位置・重ね順・色・切り出し・変形を表す。ゲーム側がGPUを知らずに表示を指定できる描画データ。
#[derive(Clone, Debug, PartialEq)]
pub struct Sprite {
    pub texture: TextureId,          // 表示する画像
    pub position: [f32; 2],          // 論理画面上の左上座標
    pub size: [f32; 2],              // 論理画面上の幅と高さ
    pub layer: i32,                  // 描画順
    pub tint: [f32; 4],              // 色と不透明度
    pub visible: bool,               // 表示するか
    pub quad: Option<[[f32; 2]; 4]>, // 変形後の四隅（左上・左下・右上・右下）
    pub uv_rect: [f32; 4],           // 画像の切り出し範囲（左・上・右・下）
}

/// 論理画面サイズ・消去色・スプライトを一画面分にまとめる。Canvas描画とオフスクリーン検証に同じ入力を使う。
#[derive(Clone, Debug)]
pub struct Frame {
    pub logical_size: [f32; 2], // 基準となる画面サイズ
    pub clear_color: [f64; 4],  // 背景の消去色
    pub sprites: Vec<Sprite>,   // 描画する画像の一覧
}

impl Frame {
    /// GPUへ渡す前に、画面・座標・色・UVの有効な範囲を確認する。不正な入力をGPU処理の失敗として扱わないための境界検査。
    pub fn validate(&self) -> Result<(), &'static str> {
        if self
            .logical_size
            .iter()
            .any(|v| !v.is_finite() || *v <= 0.0)
        {
            return Err("Scene dimensions must be positive and finite");
        }
        if self
            .clear_color
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err("Clear color must be between zero and one");
        }
        for sprite in &self.sprites {
            if sprite.position.iter().any(|v| !v.is_finite())
                || sprite.size.iter().any(|v| !v.is_finite() || *v <= 0.0)
                || sprite
                    .tint
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err("Invalid sprite position, size or tint");
            }
            if sprite
                .quad
                .is_some_and(|quad| quad.iter().flatten().any(|v| !v.is_finite()))
                || sprite
                    .uv_rect
                    .iter()
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                || sprite.uv_rect[0] > sprite.uv_rect[2]
                || sprite.uv_rect[1] > sprite.uv_rect[3]
            {
                return Err("Invalid transformed corners or texture crop");
            }
        }
        Ok(())
    }
}

/// 縦横比を保って論理画面が収まる表示領域を計算する。余白を中央に揃え、描画とポインター座標変換の基準にする。
pub fn viewport(surface: [u32; 2], logical: [f32; 2]) -> [f32; 4] {
    let scale = (surface[0] as f32 / logical[0]).min(surface[1] as f32 / logical[1]);
    let width = logical[0] * scale;
    let height = logical[1] * scale;
    [
        (surface[0] as f32 - width) / 2.0,
        (surface[1] as f32 - height) / 2.0,
        width,
        height,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_preserves_aspect_ratio_and_centers_scene() {
        assert_eq!(
            viewport([1280, 720], [1280.0, 720.0]),
            [0.0, 0.0, 1280.0, 720.0]
        );
        assert_eq!(
            viewport([720, 720], [1280.0, 720.0]),
            [0.0, 157.5, 720.0, 405.0]
        );
    }

    #[test]
    fn rejects_invalid_scene_dimensions() {
        let frame = Frame {
            logical_size: [f32::NAN, 720.0],
            clear_color: [0.0; 4],
            sprites: vec![],
        };
        assert!(frame.validate().is_err());
    }
}
