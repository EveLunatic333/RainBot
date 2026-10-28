use image::imageops;
use image::DynamicImage;
use image::GenericImageView;
use imageproc::drawing::{draw_text_mut, text_size};
use rusttype::{Font, Scale};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;
use tokio::fs;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BoxConfig {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AvatarBoxConfig {
    pub x: i64,
    pub y: i64,
    pub w: u32,
    pub h: u32,
    pub shape: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RolesConfig {
    pub x: i32,
    pub y: i32,
    pub max_w: Option<i32>,
    pub spacing_y: i32,
    pub size: f32,
    pub icon_size: u32,
    pub max_roles: usize,
    pub align: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TextConfig {
    pub x: i32,
    pub y: i32,
    pub max_w: i32,
    pub size: f32,
    pub color: Option<[u8; 4]>,
    pub align: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TemplateConfig {
    pub priority: i32,
    pub role_id: u64,
    pub image: String,
    pub font: String,
    pub char_box: BoxConfig,
    pub equip_box: BoxConfig,
    pub avatar_box: Option<AvatarBoxConfig>,
    pub roles_list: Option<RolesConfig>,
    pub texts: HashMap<String, TextConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HunterCardJson {
    pub default_template: String,
    pub templates: HashMap<String, TemplateConfig>,
}

pub struct RoleRenderData {
    pub name: String,
    pub color: image::Rgba<u8>,
    pub icon_bytes: Option<Vec<u8>>,
}

pub struct HunterCardEngine;

impl HunterCardEngine {
    fn draw_wrapped_text(
        img: &mut DynamicImage,
        font: &Font,
        color: image::Rgba<u8>,
        text: &str,
        x: i32,
        y: i32,
        max_w: i32,
        size: f32,
        align: &str,
    ) {
        let scale = Scale::uniform(size);
        let line_height = (size * 1.2) as i32;
        let mut current_y = y;

        for paragraph in text.split('\n') {
            let mut current_line = String::new();
            for word in paragraph.split_whitespace() {
                let test_line = if current_line.is_empty() {
                    word.to_string()
                } else {
                    format!("{} {}", current_line, word)
                };
                
                let (w, _) = text_size(scale, font, &test_line);

                if w as i32 > max_w && !current_line.is_empty() {
                    let (line_w, _) = text_size(scale, font, &current_line);
                    let draw_x = match align {
                        "center" => x + (max_w - line_w as i32) / 2,
                        "right" => x + (max_w - line_w as i32),
                        _ => x,
                    };
                    draw_text_mut(img, color, draw_x, current_y, scale, font, &current_line);
                    current_line = word.to_string();
                    current_y += line_height;
                } else {
                    current_line = test_line;
                }
            }
            if !current_line.is_empty() {
                let (line_w, _) = text_size(scale, font, &current_line);
                let draw_x = match align {
                    "center" => x + (max_w - line_w as i32) / 2,
                    "right" => x + (max_w - line_w as i32),
                    _ => x,
                };
                draw_text_mut(img, color, draw_x, current_y, scale, font, &current_line);
                current_y += line_height;
            }
        }
    }

    pub async fn generate(
        user_roles: Vec<u64>,
        dynamic_data: HashMap<String, String>,
        char_bytes: Option<Vec<u8>>,
        equip_bytes: Option<Vec<u8>>,
        avatar_bytes: Option<Vec<u8>>,
        roles_data: Vec<RoleRenderData>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let config_path = Path::new(".").join("static").join("huntercard.json");
        let config_str = fs::read_to_string(&config_path)
            .await
            .unwrap_or_else(|_| "{}".to_string());
        let config: HunterCardJson = serde_json::from_str(&config_str)?;

        let mut selected_template = config
            .templates
            .get(&config.default_template)
            .cloned()
            .ok_or("Default template not found!")?;
        
        let mut current_prio = selected_template.priority;

        for (_, tmpl) in config.templates.iter() {
            if user_roles.contains(&tmpl.role_id) && tmpl.priority > current_prio {
                selected_template = tmpl.clone();
                current_prio = tmpl.priority;
            }
        }

        let template_bytes = fs::read(
            Path::new(".")
                .join("icon")
                .join("HunterCard")
                .join(&selected_template.image),
        )
        .await?;
        
        let font_bytes = fs::read(
            Path::new(".")
                .join("icon")
                .join("font")
                .join(&selected_template.font),
        )
        .await?;

        let result = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
            let img_template = image::load_from_memory(&template_bytes)?;
            let font = Font::try_from_vec(font_bytes).ok_or("Failed to load font")?;

            let (width, height) = img_template.dimensions();
            let mut final_img = DynamicImage::new_rgba8(width, height);

            if let Some(bytes) = char_bytes {
                if let Ok(c_img) = image::load_from_memory(&bytes) {
                    let box_cfg = &selected_template.char_box;
                    let resized = c_img.resize_exact(
                        box_cfg.w,
                        box_cfg.h,
                        imageops::FilterType::Lanczos3,
                    );
                    imageops::overlay(&mut final_img, &resized, box_cfg.x, box_cfg.y);
                }
            }

            if let Some(bytes) = equip_bytes {
                if let Ok(e_img) = image::load_from_memory(&bytes) {
                    let box_cfg = &selected_template.equip_box;
                    let resized = e_img.resize_exact(
                        box_cfg.w,
                        box_cfg.h,
                        imageops::FilterType::Lanczos3,
                    );
                    imageops::overlay(&mut final_img, &resized, box_cfg.x, box_cfg.y);
                }
            }

            imageops::overlay(&mut final_img, &img_template, 0, 0);

            if let Some(bytes) = avatar_bytes {
                if let Some(box_cfg) = &selected_template.avatar_box {
                    if let Ok(a_img) = image::load_from_memory(&bytes) {
                        let mut resized = a_img.resize_exact(
                            box_cfg.w,
                            box_cfg.h,
                            imageops::FilterType::Lanczos3,
                        );
                        
                        let shape = box_cfg.shape.as_deref().unwrap_or("square");
                        if shape != "square" {
                            let mut rgba = resized.to_rgba8();
                            let w = box_cfg.w as f32;
                            let h = box_cfg.h as f32;
                            let cx = w / 2.0;
                            let cy = h / 2.0;
                            let r = w.min(h) / 2.0;

                            for (x, y, pixel) in rgba.enumerate_pixels_mut() {
                                let dx = x as f32 - cx + 0.5;
                                let dy = y as f32 - cy + 0.5;

                                if shape == "circle" {
                                    if dx * dx + dy * dy > r * r {
                                        pixel[3] = 0;
                                    }
                                } else if shape == "hexagon" {
                                    let max_val = dy.abs().max(dy.abs() * 0.5 + dx.abs() * 0.866025);
                                    if max_val > r {
                                        pixel[3] = 0;
                                    }
                                }
                            }
                            resized = DynamicImage::ImageRgba8(rgba);
                        }
                        imageops::overlay(&mut final_img, &resized, box_cfg.x, box_cfg.y);
                    }
                }
            }

            if let Some(roles_cfg) = &selected_template.roles_list {
                let mut current_y = roles_cfg.y;
                let align = roles_cfg.align.as_deref().unwrap_or("left");
                let max_w = roles_cfg.max_w.unwrap_or(250);

                for role in roles_data.iter().take(roles_cfg.max_roles) {
                    let scale = Scale::uniform(roles_cfg.size);
                    let (text_w, _) = text_size(scale, &font, &role.name);
                    let icon_offset = if role.icon_bytes.is_some() {
                        roles_cfg.icon_size as i32 + 8
                    } else {
                        0
                    };
                    let total_w = icon_offset + text_w as i32;

                    let start_x = match align {
                        "center" => roles_cfg.x + (max_w - total_w) / 2,
                        "right" => roles_cfg.x + (max_w - total_w),
                        _ => roles_cfg.x,
                    };

                    let mut current_x = start_x;

                    if let Some(icon_bytes) = &role.icon_bytes {
                        if let Ok(icon_img) = image::load_from_memory(icon_bytes) {
                            let resized_icon = icon_img.resize_exact(
                                roles_cfg.icon_size,
                                roles_cfg.icon_size,
                                imageops::FilterType::Lanczos3,
                            );
                            imageops::overlay(
                                &mut final_img,
                                &resized_icon,
                                current_x as i64,
                                current_y as i64,
                            );
                            current_x += icon_offset;
                        }
                    }

                    Self::draw_wrapped_text(
                        &mut final_img,
                        &font,
                        role.color,
                        &role.name,
                        current_x,
                        current_y,
                        max_w,
                        roles_cfg.size,
                        "left",
                    );

                    current_y += roles_cfg.spacing_y;
                }
            }

            for (key, text_cfg) in selected_template.texts.iter() {
                if let Some(text_value) = dynamic_data.get(key) {
                    let rgba_arr = text_cfg.color.unwrap_or([255, 255, 255, 255]);
                    let color = image::Rgba(rgba_arr);
                    let alignment = text_cfg.align.as_deref().unwrap_or("left");

                    Self::draw_wrapped_text(
                        &mut final_img,
                        &font,
                        color,
                        text_value,
                        text_cfg.x,
                        text_cfg.y,
                        text_cfg.max_w,
                        text_cfg.size,
                        alignment,
                    );
                }
            }

            let mut buffer = Cursor::new(Vec::new());
            final_img.write_to(&mut buffer, image::ImageFormat::Jpeg)?;
            Ok(buffer.into_inner())
        })
        .await??;

        Ok(result)
    }
}