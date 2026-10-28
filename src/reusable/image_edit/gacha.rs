use crate::material::ItemPedia;
use image::imageops::{resize, FilterType};
use image::{ImageBuffer, Rgb, RgbImage};
use rusttype::Font;
use std::io::Cursor;
use std::path::Path;
use super::super::bitwise::ItemCode;
use super::CustomImageError;
use super::Images;

#[derive(Clone)]
pub struct GachaData {
    pub result: GachaR,
    pub code: ItemCode,
}

pub struct GachaImage {
    rec: Rectangle,
    font: Font<'static>,
}

#[derive(Clone, PartialEq)]
pub enum GachaR {
    R,
    SR,
    SSR,
    UR,
}

#[derive(Clone)]
pub(crate) struct Rectangle {
    pub radius: u32,
    pub off_x: u32,
    pub off_y: u32,
    pub img: ImageBuffer<Rgb<u8>, Vec<u8>>,
}

impl Rectangle {
    pub(super) fn is_in_area(&self, x: u32, y: u32) -> bool {
        let im_x: i32 = x as i32 - self.off_x as i32 - self.radius as i32;
        let im_y: i32 = y as i32 - self.off_y as i32 - self.radius as i32;
        self.radius.pow(2) as i32 >= im_x.pow(2) + im_y.pow(2)
    }

    pub(super) async fn get_rect(
        url: &str,
        radius: u32,
        off_x: u32,
        off_y: u32,
    ) -> Result<Rectangle, CustomImageError> {
        let client = reqwest::Client::builder()
            .user_agent("Mozilla/5.0")
            .build()
            .map_err(|_| CustomImageError::Custom("Failed to build HTTP client"))?;

        let img = match client.get(url).send().await {
            Ok(resp) => {
                if let Ok(bytes) = resp.bytes().await {
                    match image::load_from_memory(&bytes) {
                        Ok(i) => i
                            .resize_exact(radius * 2 + 1, radius * 2 + 1, FilterType::Nearest)
                            .to_rgb8(),
                        Err(_) => ImageBuffer::from_pixel(
                            radius * 2 + 1,
                            radius * 2 + 1,
                            Rgb([100, 100, 100]),
                        ),
                    }
                } else {
                    ImageBuffer::from_pixel(radius * 2 + 1, radius * 2 + 1, Rgb([100, 100, 100]))
                }
            }
            Err(_) => ImageBuffer::from_pixel(radius * 2 + 1, radius * 2 + 1, Rgb([100, 100, 100])),
        };
        Ok(Rectangle {
            radius,
            off_x,
            off_y,
            img,
        })
    }

    pub(super) fn get_rgb_pixel(&self, x: u32, y: u32) -> Rgb<u8> {
        let norm_x = x - self.off_x;
        let norm_y = y - self.off_y;
        self.img.get_pixel(norm_x, norm_y).to_owned()
    }
}

impl GachaImage {
    pub async fn new(url: &str) -> Result<GachaImage, CustomImageError> {
        let font = GachaImage::load_font().await;
        let rec = Rectangle::get_rect(url, 51, 338, 48).await?;
        Ok(GachaImage { rec, font })
    }

    async fn load_font() -> Font<'static> {
        let path = Path::new(".").join("icon").join("NotoSerifJP-Regular.otf");
        let data = tokio::fs::read(&path)
            .await
            .expect("❌ FONT MISSING: icon/NotoSerifJP-Regular.otf");
        Font::try_from_vec(data).expect("❌ FONT FILE CORRUPTED!")
    }

    fn get_x(&self, text: &str) -> i32 {
        let len = text.len() as i32;
        386 - len * 16 / 2
    }

    async fn url_pull(
        &self,
        gacha: &GachaData,
        pedia: &ItemPedia,
        imgg: &Images,
    ) -> Result<ImageBuffer<Rgb<u8>, Vec<u8>>, CustomImageError> {
        let bytes = match gacha.result {
            GachaR::R => &imgg.gacha.r,
            GachaR::SR => &imgg.gacha.sr,
            GachaR::SSR => &imgg.gacha.ssr,
            GachaR::UR => &imgg.gacha.ur,
        };

        let mut img = match image::load_from_memory(bytes) {
            Ok(i) => i.to_rgb8(),
            Err(_) => {
                return Err(CustomImageError::Custom(
                    "Gacha Background file inside VPS is corrupted (possibly contains HTML text).",
                ));
            }
        };

        for (x, y, px) in img.enumerate_pixels_mut() {
            if self.rec.is_in_area(x, y) {
                *px = self.rec.get_rgb_pixel(x, y);
            }
        }

        let text = match gacha.code.text(pedia) {
            Some(x) => x,
            None => {
                return Err(CustomImageError::Custom("Invalid item code"));
            }
        };

        let res = imageproc::drawing::draw_text(
            &img,
            Rgb([255, 255, 255]),
            self.get_x(&text),
            510,
            rusttype::Scale { x: 50.0, y: 50.0 },
            &self.font,
            &text,
        );

        Ok(res)
    }

    pub async fn multi_pull(
        &self,
        gachas: Vec<GachaData>,
        pedia: &ItemPedia,
        imgg: &Images,
    ) -> Result<Vec<u8>, CustomImageError> {
        let mut img: RgbImage = ImageBuffer::new(1028, 615);
        let mut all_buff = Vec::new();
        
        for i in &gachas {
            let res = self.url_pull(i, pedia, imgg).await?;
            all_buff.push(resize(&res, 258, 206, FilterType::Nearest));
        }
        
        let width = 1028u32;
        let height = 615u32;
        let step_x = 257u32;
        let step_y = 205u32;
        let cell_w = 258u32;
        let cell_h = 206u32;

        for y in 0..height {
            for x in 0..width {
                let col = (x / step_x) as usize;
                let row = (y / step_y) as usize;
                
                if row < 3 && col < 4 {
                    let index = row * 4 + col;
                    if index < all_buff.len() {
                        let src_x = x - (col as u32 * step_x);
                        let src_y = y - (row as u32 * step_y);
                        
                        if src_x < cell_w && src_y < cell_h {
                            let px = all_buff[index].get_pixel(src_x, src_y);
                            img.put_pixel(x, y, px.to_owned());
                        }
                    }
                }
            }
        }

        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Jpeg(80))?;
        Ok(bytes)
    }

    pub async fn single_pull(
        &self,
        gacha: &GachaData,
        pedia: &ItemPedia,
        imgg: &Images,
    ) -> Result<Vec<u8>, CustomImageError> {
        let img = self.url_pull(gacha, pedia, imgg).await?;
        let mut bytes = Vec::new();
        img.write_to(&mut Cursor::new(&mut bytes), image::ImageOutputFormat::Jpeg(80))?;
        Ok(bytes)
    }
}

#[derive(Debug, Clone)]
pub struct Gacha {
    pub ur: Vec<u8>,
    pub ssr: Vec<u8>,
    pub sr: Vec<u8>,
    pub r: Vec<u8>,
}

impl Gacha {
    pub async fn new() -> Result<Gacha, CustomImageError> {
        let base_path = std::path::Path::new(".").join("image");
        
        let ur = tokio::fs::read(base_path.join("ur.jpg"))
            .await
            .map_err(|_| CustomImageError::Custom("❌ FILE MISSING: image/ur.jpg"))?;
        let ssr = tokio::fs::read(base_path.join("ssr.jpg"))
            .await
            .map_err(|_| CustomImageError::Custom("❌ FILE MISSING: image/ssr.jpg"))?;
        let sr = tokio::fs::read(base_path.join("sr.jpg"))
            .await
            .map_err(|_| CustomImageError::Custom("❌ FILE MISSING: image/sr.jpg"))?;
        let r = tokio::fs::read(base_path.join("r.jpg"))
            .await
            .map_err(|_| CustomImageError::Custom("❌ FILE MISSING: image/r.jpg"))?;

        Ok(Gacha { ur, ssr, sr, r })
    }
}