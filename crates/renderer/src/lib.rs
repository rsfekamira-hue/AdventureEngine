//! 汎用スプライト描画器。ゲームの進行やブラウザUIには依存しない。
use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use render_contract::{viewport, Frame, Sprite, TextureId};
use wgpu::util::DeviceExt;

/// スプライトをGPUへ送る際の座標・UV・色をまとめる。シェーダーの頂点入力と同じメモリ配置で渡すための内部型。
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 2], // GPU上の頂点座標
    uv: [f32; 2],       // 画像内の参照位置
    tint: [f32; 4],     // 色と不透明度
}

/// GPU画像と、その画像を参照するBindGroupを一緒に保持する。描画中に画像資源が解放されないよう寿命を所有する。
struct Texture {
    _texture: wgpu::Texture,  // GPU画像を保持する
    binding: wgpu::BindGroup, // 描画時に参照する画像とサンプラー
}

/// 単体スプライト試作用のGPU資源と描画手順を所有する。ゲーム進行・ファイル形式・Canvasの管理から独立してFrameを描く。
pub struct Renderer {
    device: wgpu::Device,                  // GPU資源を作る装置
    queue: wgpu::Queue,                    // 描画命令の送信先
    layout: wgpu::BindGroupLayout,         // 画像とサンプラーの配置規則
    pipeline: wgpu::RenderPipeline,        // 描画手順
    textures: HashMap<TextureId, Texture>, // 登録済みの画像
    format: wgpu::TextureFormat,           // 出力先の色形式
}

impl Renderer {
    /// 指定GPUと出力形式から装置・画像配置規則・描画手順を作る。シェーダーやパイプラインの不正を初期化時に検出する。
    pub async fn new(adapter: &wgpu::Adapter, format: wgpu::TextureFormat) -> Result<Self, String> {
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("Sprite device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::downlevel_defaults()
                        .using_resolution(adapter.limits()),
                    memory_hints: wgpu::MemoryHints::default(),
                },
                None,
            )
            .await
            .map_err(|error| error.to_string())?;

        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Sprite texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline = Self::create_pipeline(&device, &layout, format);
        if let Some(error) = device.pop_error_scope().await {
            return Err(error.to_string());
        }
        Ok(Self {
            device,
            queue,
            layout,
            pipeline,
            textures: HashMap::new(),
            format,
        })
    }

    /// 頂点入力・画像参照・シェーダー・透明度合成を一つの描画手順にする。出力形式に合わせてCanvasと画像検証で利用できるようにする。
    fn create_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        format: wgpu::TextureFormat,
    ) -> wgpu::RenderPipeline {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Sprite shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("sprite.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Sprite pipeline layout"),
            bind_group_layouts: &[layout],
            push_constant_ranges: &[],
        });
        const ATTRIBUTES: [wgpu::VertexAttribute; 3] =
            wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Sprite pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &ATTRIBUTES,
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        })
    }

    /// Canvas接続などに必要なGPU装置を読み取り専用で公開する。呼出し側が同じ装置で描画先を設定できるようにする。
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// 復号済みのRGBA画像を指定IDでGPUへ登録する。ファイル形式の解釈は呼出し側に任せ、寸法とバイト数をこの境界で確認する。
    pub fn upload_rgba(
        &mut self,
        id: TextureId,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), String> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if width == 0
            || height == 0
            || width > limit
            || height > limit
            || u64::from(width) * u64::from(height) * 4 != pixels.len() as u64
        {
            return Err("Invalid RGBA texture dimensions or byte count".into());
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Sprite texture"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Sprite sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let binding = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Sprite binding"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        self.textures.insert(
            id,
            Texture {
                _texture: texture,
                binding,
            },
        );
        Ok(())
    }

    /// 指定IDの画像資源を登録から外す。検証用画像など、一時的な素材を利用後に解放するための操作。
    pub fn remove_texture(&mut self, id: TextureId) {
        self.textures.remove(&id);
    }

    /// 標準のパイプラインでFrameを指定の描画先へ描く。Canvas固有の処理は接続側へ任せる。
    pub fn draw(
        &self,
        frame: &Frame,
        view: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Result<(), String> {
        self.draw_with_pipeline(frame, view, size, &self.pipeline)
    }

    /// 表示するスプライトを重ね順に並べ、頂点と画像参照を使って描く。オフスクリーンでも出力形式に合うパイプラインを選べる共通処理。
    fn draw_with_pipeline(
        &self,
        frame: &Frame,
        view: &wgpu::TextureView,
        size: [u32; 2],
        pipeline: &wgpu::RenderPipeline,
    ) -> Result<(), String> {
        // 表示する画像を層順に並べ、頂点へ変換してからGPUへ送る。
        frame.validate()?;
        if size.contains(&0) {
            return Ok(());
        }
        let mut sprites: Vec<_> = frame
            .sprites
            .iter()
            .filter(|sprite| sprite.visible && sprite.tint[3] > 0.0)
            .collect();
        // 同じ層の画像は、渡された順番のまま描く。
        sprites.sort_by_key(|sprite| sprite.layer);
        let mut vertices = Vec::with_capacity(sprites.len() * 6);
        for sprite in &sprites {
            if !self.textures.contains_key(&sprite.texture) {
                return Err(format!("Texture {} is not loaded", sprite.texture.0));
            }
            vertices.extend(Self::vertices(sprite, frame.logical_size));
        }
        let vertex_buffer = (!vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("Frame vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Sprite commands"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Sprite pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: frame.clear_color[0],
                            g: frame.clear_color[1],
                            b: frame.clear_color[2],
                            a: frame.clear_color[3],
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let [x, y, width, height] = viewport(size, frame.logical_size);
            pass.set_viewport(x, y, width, height, 0.0, 1.0);
            pass.set_pipeline(pipeline);
            if let Some(buffer) = &vertex_buffer {
                pass.set_vertex_buffer(0, buffer.slice(..));
                for (index, sprite) in sprites.iter().enumerate() {
                    pass.set_bind_group(0, &self.textures[&sprite.texture].binding, &[]);
                    let start = index as u32 * 6;
                    pass.draw(start..start + 6, 0..1);
                }
            }
        }
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    /// スプライトの四隅・切り出し・色を二つの三角形に変換する。論理座標をGPUの座標系へ揃え、矩形と変形済み画像を同じ描画手順で扱う。
    fn vertices(sprite: &Sprite, logical: [f32; 2]) -> [Vertex; 6] {
        let [x, y] = sprite.position;
        let [width, height] = sprite.size;
        let corners = sprite.quad.unwrap_or([
            [x, y],
            [x, y + height],
            [x + width, y],
            [x + width, y + height],
        ]);
        let positions =
            corners.map(|[x, y]| [x / logical[0] * 2.0 - 1.0, 1.0 - y / logical[1] * 2.0]);
        let [left, top, right, bottom] = sprite.uv_rect;
        let vertex = |position, uv| Vertex {
            position,
            uv,
            tint: sprite.tint,
        };
        [
            vertex(positions[0], [left, top]),
            vertex(positions[1], [left, bottom]),
            vertex(positions[2], [right, top]),
            vertex(positions[2], [right, top]),
            vertex(positions[1], [left, bottom]),
            vertex(positions[3], [right, bottom]),
        ]
    }

    /// Frameを画像へ描いてGPUからRGBAを読み戻す。転送用の行間余白を取り除き、比較しやすい連続したピクセル列を返す。
    pub async fn capture(&self, frame: &Frame, size: [u32; 2]) -> Result<Vec<u8>, String> {
        let [width, height] = size;
        if width == 0 || height == 0 || width > 4096 || height > 4096 {
            return Err("Capture dimensions must be between 1 and 4096".into());
        }
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Verification target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        if self.format == format {
            self.draw(frame, &view, size)?;
        } else {
            let pipeline = Self::create_pipeline(&self.device, &self.layout, format);
            self.draw_with_pipeline(frame, &view, size, &pipeline)?;
        }
        let stride = (width * 4).div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Verification readback"),
            size: u64::from(stride) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Readback commands"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
        let (sender, receiver) = futures_channel::oneshot::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = sender.send(result);
            });
        #[cfg(not(target_arch = "wasm32"))]
        self.device.poll(wgpu::Maintain::Wait);
        receiver
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        let mapped = buffer.slice(..).get_mapped_range();
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for row in mapped.chunks_exact(stride as usize) {
            pixels.extend_from_slice(&row[..width as usize * 4]);
        }
        drop(mapped);
        buffer.unmap();
        Ok(pixels)
    }

    /// 検証用の小さな画像を登録して実GPUの描画結果を調べる。実素材を変更せず、検証後は一時画像を登録から外す。
    pub async fn verify(&mut self) -> Result<String, String> {
        let red = TextureId(u32::MAX);
        let blue = TextureId(u32::MAX - 1);
        if self.textures.contains_key(&red) || self.textures.contains_key(&blue) {
            return Err("Verification texture IDs are already in use".into());
        }
        self.upload_rgba(red, 1, 1, &[255, 0, 0, 255])?;
        self.upload_rgba(blue, 1, 1, &[0, 0, 255, 255])?;
        let result = self.verify_frames(red, blue).await;
        self.remove_texture(red);
        self.remove_texture(blue);
        result
    }

    /// 検証用画像の重ね順・透明度・位置・サイズ・変形・切り出しを変えて確認する。CPU上の計算だけでなくGPUの出力を比較する。
    async fn verify_frames(&mut self, red: TextureId, blue: TextureId) -> Result<String, String> {
        let sprite = |texture, layer, opacity| Sprite {
            texture,
            position: [0.0, 0.0],
            size: [4.0, 4.0],
            layer,
            tint: [1.0, 1.0, 1.0, opacity],
            visible: true,
            quad: None,
            uv_rect: [0.0, 0.0, 1.0, 1.0],
        };
        let mut frame = Frame {
            logical_size: [4.0, 4.0],
            clear_color: [0.0, 0.0, 0.0, 1.0],
            sprites: vec![sprite(blue, 2, 0.5), sprite(red, 1, 1.0)],
        };
        let pixels = self.capture(&frame, [4, 4]).await?;
        Self::expect_pixel(&pixels, 0, [127, 0, 128, 255], "alpha and sorted layers")?;
        frame.sprites[0].layer = 0;
        Self::expect_pixel(
            &self.capture(&frame, [4, 4]).await?,
            0,
            [255, 0, 0, 255],
            "layer change",
        )?;
        frame.sprites[1].visible = false;
        frame.sprites[0].tint[3] = 0.0;
        Self::expect_pixel(
            &self.capture(&frame, [4, 4]).await?,
            0,
            [0, 0, 0, 255],
            "hidden and transparent sprites",
        )?;
        frame.sprites[0].tint[3] = 1.0;
        frame.sprites[0].position = [2.0, 0.0];
        frame.sprites[0].size = [2.0, 4.0];
        let pixels = self.capture(&frame, [4, 4]).await?;
        Self::expect_pixel(&pixels, 0, [0, 0, 0, 255], "position")?;
        Self::expect_pixel(&pixels, 3, [0, 0, 255, 255], "size")?;
        frame.sprites[0].position = [0.0, 0.0];
        frame.sprites[0].size = [4.0, 4.0];
        let pixels = self.capture(&frame, [8, 4]).await?;
        Self::expect_pixel(&pixels, 0, [0, 0, 0, 255], "letterbox margin")?;
        Self::expect_pixel(&pixels, 3, [0, 0, 255, 255], "resized scene")?;
        // Texture alpha must work independently of per-sprite opacity.
        self.upload_rgba(blue, 1, 1, &[0, 0, 255, 0])?;
        Self::expect_pixel(
            &self.capture(&frame, [4, 4]).await?,
            0,
            [0, 0, 0, 255],
            "texture alpha",
        )?;
        self.upload_rgba(blue, 2, 1, &[255, 0, 0, 255, 0, 255, 0, 255])?;
        frame.sprites[0].quad = Some([[1.0, 1.0], [1.0, 3.0], [3.0, 1.0], [3.0, 3.0]]);
        frame.sprites[0].uv_rect = [0.5, 0.0, 1.0, 1.0];
        let pixels = self.capture(&frame, [4, 4]).await?;
        Self::expect_pixel(&pixels, 0, [0, 0, 0, 255], "transformed corners")?;
        Self::expect_pixel(&pixels, 10, [0, 255, 0, 255], "cropped UVs")?;
        Ok("GPU verification passed: alpha, layers, visibility, position, size, resize, corners, crop".into())
    }

    /// 読み戻した指定ピクセルを期待値と比較する。GPU計算の丸めによる小さな色差を許容して判定する。
    fn expect_pixel(
        pixels: &[u8],
        index: usize,
        expected: [u8; 4],
        name: &str,
    ) -> Result<(), String> {
        let actual = &pixels[index * 4..index * 4 + 4];
        if actual.iter().zip(expected).any(|(a, b)| a.abs_diff(b) > 2) {
            return Err(format!("{name}: expected {expected:?}, got {actual:?}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "Requires a GPU adapter; run cargo test -p sprite-renderer -- --ignored"]
    fn gpu_pixels_match_expected_compositing_and_resize() {
        pollster::block_on(async {
            let instance = wgpu::Instance::default();
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .expect("GPU adapter");
            let mut renderer = Renderer::new(&adapter, wgpu::TextureFormat::Rgba8Unorm)
                .await
                .unwrap();
            renderer.verify().await.unwrap();
            assert!(renderer.upload_rgba(TextureId(10), 1, 1, &[0]).is_err());
            let frame = Frame {
                logical_size: [4.0, 4.0],
                clear_color: [0.0; 4],
                sprites: vec![Sprite {
                    texture: TextureId(20),
                    position: [0.0; 2],
                    size: [1.0; 2],
                    layer: 0,
                    tint: [1.0; 4],
                    visible: true,
                    quad: None,
                    uv_rect: [0.0, 0.0, 1.0, 1.0],
                }],
            };
            assert!(renderer
                .capture(&frame, [4, 4])
                .await
                .unwrap_err()
                .contains("not loaded"));
        });
    }
}
