use image::codecs::gif::{GifDecoder, GifEncoder, Repeat};
use image::imageops::FilterType;
use image::{Frame, ImageBuffer, Rgba, AnimationDecoder, RgbaImage};
use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use super::super::component::bounty::TitleImage;
use super::CustomImageError;
use super::gacha::Rectangle;
use gifski::Settings;
use imgref::ImgVec;
use rgb::RGBA8;
use lazy_static::lazy_static;
use rayon::prelude::*;
use rayon::iter::IntoParallelRefIterator;

lazy_static! {
    static ref AVATAR_CACHE: Mutex<HashMap<String, Arc<RgbaImage>>> = Mutex::new(HashMap::new());
}

impl TitleImage {
    fn find_image_path(&self) -> Option<PathBuf> {
        let raw_string = self.url.split('/').last().unwrap_or(&self.url);
        let clean_string = raw_string.split('?').next().unwrap_or(raw_string);
        let target_name = Path::new(clean_string)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let dir_path = Path::new(".").join("image").join("bounty_title");

        if let Ok(entries) = fs::read_dir(&dir_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Some(stem) = path.file_stem() {
                        if stem.to_string_lossy() == target_name {
                            return Some(path);
                        }
                    }
                }
            }
        }
        None
    }

    fn get_image_extension(&self) -> Option<String> {
        let path = self.find_image_path()?;
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
    }

    async fn get_rect(&self, url: &str) -> Result<Rectangle, CustomImageError> {
        Ok(Rectangle::get_rect(url, self.diameter / 2, self.x_start, self.y_start).await?)
    }

    async fn load_avatar_cached(&self, url: &str) -> Result<Arc<RgbaImage>, CustomImageError> {
        let cache_key = format!("{}_{}", url, self.diameter);

        {
            let cache = AVATAR_CACHE.lock().unwrap();
            if let Some(cached) = cache.get(&cache_key) {
                tracing::debug!("Avatar cache HIT for {}", cache_key);
                return Ok(cached.clone());
            }
        }
        
        tracing::debug!("Avatar cache MISS for {}, downloading...", cache_key);

        let response = reqwest::get(url).await
            .map_err(|e| std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to download avatar: {}", e)
            ))?;
        
        let bytes = response.bytes().await
            .map_err(|e| std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Failed to read avatar bytes: {}", e)
            ))?;

        let avatar = image::load_from_memory(&bytes)
            .map_err(|e| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to decode avatar: {}", e)
            ))?
            .resize_exact(self.diameter, self.diameter, FilterType::Lanczos3)
            .into_rgba8();
        
        let avatar_arc = Arc::new(avatar);

        {
            let mut cache = AVATAR_CACHE.lock().unwrap();
            if cache.len() >= 100 {
                if let Some(oldest_key) = cache.keys().next().cloned() {
                    cache.remove(&oldest_key);
                    tracing::debug!("Evicted oldest avatar from cache: {}", oldest_key);
                }
            }
            cache.insert(cache_key.clone(), avatar_arc.clone());
        }
        
        Ok(avatar_arc)
    }

    pub async fn title(&self, url: &str, is_preview: bool) -> Result<Vec<u8>, CustomImageError> {
        if self.animated {
            match self.get_image_extension().as_deref() {
                Some("webp") => self.title_animated_webp(url, is_preview).await,
                Some("gif")  => self.title_animated_gif(url).await,
                _            => self.title_static_image(url).await,
            }
        } else {
            self.title_static_image(url).await
        }
    }

    async fn title_static_image(&self, url: &str) -> Result<Vec<u8>, CustomImageError> {
        let path_opt = self.find_image_path();
        let target_w = self.bg_width.unwrap_or(1150);
        let target_h = self.bg_height.unwrap_or(658);
        let default_color = Rgba([200u8, 50u8, 50u8, 255u8]);
        
        let mut bg: ImageBuffer<Rgba<u8>, Vec<u8>> = match path_opt {
            Some(path) => match image::open(&path) {
                Ok(img) => {
                    if img.width() != target_w || img.height() != target_h {
                        img.resize_exact(target_w, target_h, FilterType::Lanczos3).into_rgba8()
                    } else {
                        img.into_rgba8()
                    }
                }
                Err(_) => ImageBuffer::from_pixel(target_w, target_h, default_color),
            },
            None => ImageBuffer::from_pixel(target_w, target_h, default_color),
        };

        let rec = self.get_rect(url).await?;

        for (x, y, px) in bg.enumerate_pixels_mut() {
            if rec.is_in_area(x, y) {
                let avatar_px = rec.get_rgb_pixel(x, y);
                let banner_px = *px;
                let alpha = banner_px[3] as f32 / 255.0;
                let inv_alpha = 1.0 - alpha;
                
                let r = (banner_px[0] as f32 * alpha + avatar_px[0] as f32 * inv_alpha) as u8;
                let g = (banner_px[1] as f32 * alpha + avatar_px[1] as f32 * inv_alpha) as u8;
                let b = (banner_px[2] as f32 * alpha + avatar_px[2] as f32 * inv_alpha) as u8;
                
                *px = Rgba([r, g, b, 255]); 
            }
        }

        let mut bytes = Vec::new();
        let bg_rgb = image::DynamicImage::ImageRgba8(bg).into_rgb8();
        bg_rgb.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Jpeg(80))?;
        Ok(bytes)
    }

    async fn title_animated_gif(&self, url: &str) -> Result<Vec<u8>, CustomImageError> {
        let path = match self.find_image_path() {
            Some(p) => p,
            None => return self.title_static_image(url).await,
        };

        let file = fs::File::open(&path)?;
        let decoder = GifDecoder::new(file)?;
        let rec = self.get_rect(url).await?;

        let target_w = self.bg_width.unwrap_or(1150);
        let target_h = self.bg_height.unwrap_or(658);

        let mut output_frames = Vec::new();

        for frame_result in decoder.into_frames() {
            if let Ok(frame) = frame_result {
                let delay = frame.delay();
                let mut bg = frame.into_buffer();

                if bg.width() != target_w || bg.height() != target_h {
                    bg = image::imageops::resize(&bg, target_w, target_h, FilterType::Lanczos3);
                }

                for (x, y, px) in bg.enumerate_pixels_mut() {
                    if rec.is_in_area(x, y) {
                        let avatar_px = rec.get_rgb_pixel(x, y);
                        let banner_px = *px; 
                        let alpha = banner_px[3] as f32 / 255.0;
                        let inv_alpha = 1.0 - alpha;
                        
                        let r = (banner_px[0] as f32 * alpha + avatar_px[0] as f32 * inv_alpha) as u8;
                        let g = (banner_px[1] as f32 * alpha + avatar_px[1] as f32 * inv_alpha) as u8;
                        let b = (banner_px[2] as f32 * alpha + avatar_px[2] as f32 * inv_alpha) as u8;
                        
                        *px = Rgba([r, g, b, 255]);
                    }
                }
                output_frames.push(Frame::from_parts(bg, 0, 0, delay));
            }
        }

        let mut bytes = Vec::new();
        {
            let mut encoder = GifEncoder::new(&mut bytes);
            encoder.set_repeat(Repeat::Infinite).unwrap_or_default();
            encoder.encode_frames(output_frames)?;
        }

        Ok(bytes)
    }

        async fn title_animated_webp(&self, url: &str, is_preview: bool) -> Result<Vec<u8>, CustomImageError> {
        let start_time = std::time::Instant::now();
        
        let path = match self.find_image_path() {
            Some(p) => p,
            None => return self.title_static_image(url).await,
        };

        let rec = self.get_rect(url).await?;
        let avatar = self.load_avatar_cached(url).await?;

        let webp_bytes = Arc::new(fs::read(&path)?);

        let (width, height) = {
            let decoder = webp_animation::Decoder::new(&webp_bytes[..])
                .map_err(|e| std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("WebP decode error: {:?}", e)
                ))?;
            let mut iter = decoder.into_iter().peekable();
            iter.peek()
                .map(|f| f.dimensions())
                .ok_or_else(|| std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "WebP file contains no frames"
                ))?
        };

        let scale = if is_preview { 0.6 } else { 0.8 };
        let output_w = (width as f32 * scale) as u32;
        let output_h = (height as f32 * scale) as u32;
        let quality = if is_preview { 60 } else { 75 };

        tracing::info!(
            "⚡ Processing WebP with PARALLEL mode (source: {}x{}, output: {}x{}, quality: {}, preview: {})",
            width, height, output_w, output_h, quality, is_preview
        );

        let settings = Settings {
            width: Some(output_w),
            height: Some(output_h),
            quality,
            fast: true,
            repeat: gifski::Repeat::Infinite,
        };

        let (collector, writer) = gifski::new(settings)
            .map_err(|e| std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Gifski init failed: {:?}", e)
            ))?;

        let collector_handle = std::thread::spawn({
            let webp_bytes = webp_bytes.clone();
            let avatar = avatar.clone();
            let rec_clone = rec.clone();
            
            move || -> Result<usize, std::io::Error> {
                let decoder = webp_animation::Decoder::new(&webp_bytes[..])
                    .map_err(|e| std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("WebP re-decode error: {:?}", e)
                    ))?;

                let frames_data: Vec<(usize, Vec<u8>, i32, u32, u32)> = decoder
                    .into_iter()
                    .enumerate()
                    .map(|(i, frame)| {
                        let (w, h) = frame.dimensions();
                        (i, frame.data().to_vec(), frame.timestamp(), w, h)
                    })
                    .collect();

                let total_frames = frames_data.len();
                tracing::info!("📊 Extracted {} frames, starting parallel processing...", total_frames);

                let parallel_start = std::time::Instant::now();

                let mut processed_frames: Vec<(usize, Vec<RGBA8>, f64)> = frames_data
                    .par_iter()
                    .map(|(i, frame_data, timestamp, w, h)| {
                        let _avatar_local = avatar.clone();
                        let rec_local = rec_clone.clone();

                        let mut canvas = RgbaImage::new(output_w, output_h);

                        let timestamp_secs = *timestamp as f64 / 1000.0;

                        for pixel in canvas.pixels_mut() {
                            *pixel = image::Rgba([0, 0, 0, 0]);
                        }

                        let needs_resize = *w != output_w || *h != output_h;
                        
                        if needs_resize {
                            let frame_rgba: ImageBuffer<Rgba<u8>, Vec<u8>> = 
                                ImageBuffer::from_raw(*w, *h, frame_data.clone())
                                    .expect("Failed to create frame buffer");
                            
                            let resized = image::imageops::resize(&frame_rgba, output_w, output_h, FilterType::Lanczos3);
                            
                            for (x, y, px) in resized.enumerate_pixels() {
                                if x < output_w && y < output_h {
                                    *canvas.get_pixel_mut(x, y) = *px;
                                }
                            }
                        } else {
                            for py in 0..*h {
                                for px in 0..*w {
                                    let idx = ((py * w + px) * 4) as usize;
                                    if idx + 3 < frame_data.len() {
                                        *canvas.get_pixel_mut(px, py) = image::Rgba([
                                            frame_data[idx],
                                            frame_data[idx + 1],
                                            frame_data[idx + 2],
                                            frame_data[idx + 3],
                                        ]);
                                    }
                                }
                            }
                        }

                        for (x, y, px) in canvas.enumerate_pixels_mut() {
                            let (scaled_x, scaled_y) = if needs_resize {
                                (
                                    (x as f32 / output_w as f32 * width as f32) as u32,
                                    (y as f32 / output_h as f32 * height as f32) as u32,
                                )
                            } else {
                                (x, y)
                            };
                            
                            if rec_local.is_in_area(scaled_x, scaled_y) {
                                let avatar_px = rec_local.get_rgb_pixel(scaled_x, scaled_y);
                                let banner_px = *px;
                                
                                let src_a = banner_px[3] as u32;
                                
                                if src_a == 0 {
                                    continue;
                                } else if src_a == 255 {
                                    *px = banner_px;
                                } else {
                                    let inv_a = 255 - src_a;
                                    let avatar_a = 255u32;
                                    let out_a = src_a + (avatar_a * inv_a / 255);
                                    
                                    *px = Rgba([
                                        ((banner_px[0] as u32 * src_a + avatar_px[0] as u32 * inv_a) / 255) as u8,
                                        ((banner_px[1] as u32 * src_a + avatar_px[1] as u32 * inv_a) / 255) as u8,
                                        ((banner_px[2] as u32 * src_a + avatar_px[2] as u32 * inv_a) / 255) as u8,
                                        out_a.min(255) as u8,
                                    ]);
                                }
                            }
                        }

                        let pixels: Vec<RGBA8> = canvas
                            .pixels()
                            .map(|p| RGBA8::new(p[0], p[1], p[2], p[3]))
                            .collect();

                        (*i, pixels, timestamp_secs)
                    })
                    .collect();

                let parallel_elapsed = parallel_start.elapsed();
                tracing::info!(
                    "⚡ Parallel processing completed in {:.2}s ({} frames, {:.1} fps)",
                    parallel_elapsed.as_secs_f32(),
                    total_frames,
                    total_frames as f32 / parallel_elapsed.as_secs_f32()
                );

                processed_frames.sort_by_key(|(i, _, _)| *i);

                let mut frame_index = 0usize;
                for (i, pixels, timestamp_secs) in processed_frames {
                    let imgvec = ImgVec::new(pixels, output_w as usize, output_h as usize);

                    if let Err(e) = collector.add_frame_rgba(frame_index, imgvec, timestamp_secs) {
                        tracing::error!("Failed to add frame {}: {:?}", i, e);
                        break;
                    }

                    frame_index += 1;
                }

                Ok(frame_index)
            }
        });

        let mut bytes = Vec::new();
        writer.write(&mut bytes, &mut gifski::progress::NoProgress {})
            .map_err(|e| std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Gifski write failed: {:?}", e)
            ))?;

        let frame_count = match collector_handle.join() {
            Ok(Ok(fc)) => fc,
            Ok(Err(e)) => {
                tracing::error!("Collector error: {:?}", e);
                0
            }
            Err(_) => {
                tracing::error!("Collector thread panicked");
                0
            }
        };

        let elapsed = start_time.elapsed();
        tracing::info!(
            "✅ Parallel render completed in {:.2}s (frames={}, size={}KB, preview={})",
            elapsed.as_secs_f32(),
            frame_count,
            bytes.len() / 1024,
            is_preview
        );

        if bytes.is_empty() {
            return self.title_static_image(url).await;
        }

        Ok(bytes)
    }
}