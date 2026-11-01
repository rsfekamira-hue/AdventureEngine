//! 汎用2D描画。ゲームの情報・選択肢・進行規則には依存しない。
use egui::{self, vec2, Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind};
#[cfg(feature = "gpu")]
pub mod digital;
#[cfg(feature = "gpu")]
pub mod particles;
#[cfg(feature = "gpu")]
mod sprites;
pub mod textures;
use textures::TextureStore;

/// 一つの文字列区間の内容・サイズ・色をまとめる。透明な未表示文字も同じレイアウトへ含め、文字送り中の改行位置を固定する。
pub struct TextSection {
    pub text: String,
    pub size: f32,
    pub color: Color32,
}
/// 画像・文字・図形・フィルタ・シェーダーをゲームに依存しない描画指示として表す。配置処理と描画ライブラリへの実行を分けるための型。
pub enum DrawCommand {
    Rect {
        rect: Rect,
        radius: f32,
        fill: Color32,
        stroke: Stroke,
        kind: StrokeKind,
    },
    Text {
        position: egui::Pos2,
        align: Align2,
        text: String,
        font: FontId,
        color: Color32,
    },
    Line {
        points: [egui::Pos2; 2],
        stroke: Stroke,
    },
    Polygon {
        points: Vec<egui::Pos2>,
        fill: Color32,
        stroke: Stroke,
    },
    Polyline {
        points: Vec<egui::Pos2>,
        stroke: Stroke,
    },
    Sprite {
        image: String,
        rect: Rect,
        uv: Rect,
        rotation: f32,
        tint: Color32,
    },
    TextBlock {
        rect: Rect,
        id: String,
        sections: Vec<TextSection>,
        line_height: f32,
        stick_to_bottom: bool,
    },
    Filter {
        rect: Rect,
        tint: Color32,
    },
    /// egui/wgpuの専用シェーダー描画を同じ命令列へ組み込む。
    Shader(egui::PaintCallback),
    /// Definitions and time only; simulation and instance geometry stay on GPU.
    #[cfg(feature = "gpu")]
    Particles(particles::ParticleDraw),
    #[cfg(feature = "gpu")]
    Digital(digital::DigitalDraw),
}
/// 一つの描画命令に、その時点のクリップ領域を付ける。背景・本文など異なる領域の命令を同じ順序列で扱えるようにする。
pub struct DrawItem {
    pub clip: Rect,
    pub command: DrawCommand,
}
/// 一画面分の順序付き描画命令をまとめる。ゲーム描画側で作った内容をRenderer2Dへ一括で渡す。
pub struct DrawFrame {
    pub items: Vec<DrawItem>,
}
/// 一画面の描画命令を所有する記録器。必要な間だけ可変借用し、命令の追加順と描画範囲を管理する。
pub struct DrawList {
    items: Vec<DrawItem>,
    clip: Rect,
}
impl DrawList {
    /// 指定した描画可能領域から空の命令列を作る。一画面分の命令をここから記録する。
    pub fn new(clip: Rect) -> Self {
        Self {
            items: Vec::new(),
            clip,
        }
    }
    /// 指定領域と親の交差範囲で命令を記録し、終了時に元の範囲へ戻す。共有所有を作らず、子の描画へ一時的な可変借用を渡す。
    pub fn with_clip_rect(&mut self, clip: Rect, draw: impl FnOnce(&mut Self)) {
        let parent = self.clip;
        self.clip = parent.intersect(clip);
        draw(self);
        self.clip = parent;
    }
    /// 現在のクリップ領域を付けて命令を末尾へ記録する。呼出し順を描画順として保持する。
    pub fn push(&mut self, command: DrawCommand) {
        self.items.push(DrawItem {
            clip: self.clip,
            command,
        });
    }
    /// 記録した命令列をDrawFrameへ移す。命令や画像資源を複製せず、一画面の記録から描画実行へ所有権を渡す。
    pub fn finish(self) -> DrawFrame {
        DrawFrame { items: self.items }
    }
    /// 塗り・輪郭・角丸を一つの矩形命令として記録する。実際の図形生成は描画実行時に行う。
    pub fn rect(
        &mut self,
        rect: Rect,
        radius: f32,
        fill: Color32,
        stroke: Stroke,
        kind: StrokeKind,
    ) {
        self.push(DrawCommand::Rect {
            rect,
            radius,
            fill,
            stroke,
            kind,
        });
    }
    /// 輪郭を持たない矩形を記録する。塗りだけの領域を共通の矩形命令で扱う。
    pub fn rect_filled(&mut self, rect: Rect, radius: f32, fill: Color32) {
        self.rect(rect, radius, fill, Stroke::NONE, StrokeKind::Inside);
    }
    /// 塗りを持たない矩形を記録する。輪郭だけの領域も塗り付きの矩形と同じ命令形式に揃える。
    pub fn rect_stroke(&mut self, rect: Rect, radius: f32, stroke: Stroke, kind: StrokeKind) {
        self.rect(rect, radius, Color32::TRANSPARENT, stroke, kind);
    }
    /// 短い文字列の位置・揃え方・書式を記録する。文字の配置を決める側でフォント描画を実行しない。
    pub fn text(
        &mut self,
        position: egui::Pos2,
        align: Align2,
        text: impl Into<String>,
        font: FontId,
        color: Color32,
    ) {
        self.push(DrawCommand::Text {
            position,
            align,
            text: text.into(),
            font,
            color,
        });
    }
    /// 二点と線の書式を記録する。区切り線やグラフの線分を同じ汎用命令で描けるようにする。
    pub fn line_segment(&mut self, points: [egui::Pos2; 2], stroke: Stroke) {
        self.push(DrawCommand::Line { points, stroke });
    }
    /// 画像名・領域・UV・回転・乗算色と透明度を記録する。画像のGPUハンドルをゲーム描画側へ公開しない。
    pub fn sprite(&mut self, image: &str, rect: Rect, uv: Rect, rotation: f32, tint: Color32) {
        self.push(DrawCommand::Sprite {
            image: image.into(),
            rect,
            uv,
            rotation,
            tint,
        });
    }
    /// 指定領域への色重ねをフィルタ命令として記録する。雰囲気の意味を描画器に渡さず、具体的な色だけで実行する。
    pub fn filter(&mut self, rect: Rect, tint: Color32) {
        self.push(DrawCommand::Filter { rect, tint });
    }
}
/// 汎用描画命令を実行する。画像の所有・取得・寿命管理は行わない。
pub struct Renderer2D {
    context: egui::Context,
    #[cfg(feature = "gpu")]
    particles: std::cell::RefCell<particles::ParticlePainter>,
    #[cfg(feature = "gpu")]
    digital: digital::DigitalPainter,
}
impl Renderer2D {
    /// 日本語フォントを設定する。画像は描画時に借用する。
    pub fn new(ctx: &egui::Context, font: Vec<u8>) -> Self {
        #[cfg(feature = "gpu")]
        let particle_font =
            ab_glyph::FontArc::try_from_vec(font.clone()).expect("renderer requires a valid font");
        let mut fonts = egui::FontDefinitions::default();
        fonts
            .font_data
            .insert("Japanese".into(), egui::FontData::from_owned(font).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "Japanese".into());
        ctx.set_fonts(fonts);
        Self {
            context: ctx.clone(),
            #[cfg(feature = "gpu")]
            particles: std::cell::RefCell::new(particles::ParticlePainter::new(
                particle_font.clone(),
            )),
            #[cfg(feature = "gpu")]
            digital: digital::DigitalPainter::new(particle_font),
        }
    }
    /// 外部入力で表示が変わった際に次の描画を要求する。ブラウザ側へ描画ライブラリの状態を公開しない。
    pub fn request_repaint(&self) {
        self.context.request_repaint();
    }
    /// 命令順とクリップ領域を保って一画面分を描く。画像のメッシュ、文字の折返し・スクロール、専用シェーダーの実行をこの層へ集約する。
    pub fn render(&self, ui: &mut egui::Ui, frame: DrawFrame, textures: &TextureStore) {
        #[cfg(feature = "gpu")]
        let mut digital_slot = 0;
        #[cfg(feature = "gpu")]
        let digital_characters = std::sync::Arc::new(
            frame
                .items
                .iter()
                .filter_map(|item| {
                    if let DrawCommand::Digital(draw) = &item.command {
                        Some(draw)
                    } else {
                        None
                    }
                })
                .flat_map(|draw| draw.text.iter().flat_map(|text| text.text.chars()))
                .filter(|c| *c != '\n')
                .collect::<std::collections::HashSet<_>>(),
        );
        for item in frame.items {
            let painter = ui.painter().with_clip_rect(item.clip);
            match item.command {
                #[cfg(feature = "gpu")]
                DrawCommand::Digital(draw) => {
                    painter.add(egui::Shape::Callback(
                        self.digital.callback(
                            ui.ctx().screen_rect(),
                            textures
                                .gpu_format()
                                .expect("digital overlays require GPU textures"),
                            digital_slot,
                            digital_characters.clone(),
                            draw,
                        ),
                    ));
                    digital_slot += 1;
                }
                #[cfg(feature = "gpu")]
                DrawCommand::Particles(draw) => {
                    let format = textures
                        .gpu_format()
                        .expect("GPU particles require TextureStore::with_gpu");
                    for callback in
                        self.particles
                            .borrow_mut()
                            .callbacks(ui.ctx().screen_rect(), format, draw)
                    {
                        painter.add(egui::Shape::Callback(callback));
                    }
                }
                DrawCommand::Rect {
                    rect,
                    radius,
                    fill,
                    stroke,
                    kind,
                } => {
                    painter.rect(rect, radius, fill, stroke, kind);
                }
                DrawCommand::Text {
                    position,
                    align,
                    text,
                    font,
                    color,
                } => {
                    painter.text(position, align, text, font, color);
                }
                DrawCommand::Line { points, stroke } => {
                    painter.line_segment(points, stroke);
                }
                DrawCommand::Polygon {
                    points,
                    fill,
                    stroke,
                } => {
                    painter.add(egui::Shape::convex_polygon(points, fill, stroke));
                }
                DrawCommand::Polyline { points, stroke } => {
                    painter.add(egui::Shape::line(points, stroke));
                }
                DrawCommand::Filter { rect, tint } => {
                    painter.rect_filled(rect, 0.0, tint);
                }
                DrawCommand::Shader(callback) => {
                    painter.add(egui::Shape::Callback(callback));
                }
                DrawCommand::Sprite {
                    image,
                    rect,
                    uv,
                    rotation,
                    tint,
                } => {
                    if let Some(texture) = textures.get(&image) {
                        #[cfg(feature = "gpu")]
                        if let Some(image) = texture.gpu() {
                            painter.add(sprites::callback(
                                ui.ctx().screen_rect(),
                                image,
                                rect,
                                uv,
                                rotation,
                                tint,
                            ));
                            continue;
                        }
                        let mut mesh = egui::epaint::Mesh::with_texture(texture.id());
                        mesh.add_rect_with_uv(rect, uv, tint);
                        if rotation != 0.0 {
                            let center = rect.center();
                            let (sin, cos) = rotation.sin_cos();
                            for vertex in &mut mesh.vertices {
                                let p = vertex.pos - center;
                                vertex.pos =
                                    center + vec2(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
                            }
                        }
                        painter.add(egui::Shape::mesh(mesh));
                    }
                }
                DrawCommand::TextBlock {
                    rect,
                    id,
                    sections,
                    line_height,
                    stick_to_bottom,
                } => {
                    let mut job = egui::text::LayoutJob::default();
                    job.wrap.max_width = rect.width();
                    for section in sections {
                        job.append(
                            &section.text,
                            0.0,
                            egui::TextFormat {
                                font_id: FontId::proportional(section.size),
                                color: section.color,
                                line_height: Some(line_height),
                                ..Default::default()
                            },
                        );
                    }
                    let galley = ui.fonts(|fonts| fonts.layout_job(job));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.set_clip_rect(ui.clip_rect().intersect(item.clip).intersect(rect));
                        egui::ScrollArea::vertical()
                            .id_salt(id)
                            // 本文を覆うドラッグ領域で、配置元UIのクリックを奪わない。
                            // スクロールバーとホイールによるスクロールは維持する。
                            .drag_to_scroll(false)
                            .max_height(rect.height())
                            .stick_to_bottom(stick_to_bottom)
                            .show(ui, |ui| {
                                let (bounds, _) = ui.allocate_exact_size(
                                    vec2(rect.width(), galley.size().y.max(rect.height())),
                                    Sense::hover(),
                                );
                                ui.painter().galley(bounds.min, galley, Color32::WHITE);
                            });
                    });
                }
            }
        }
    }
}
