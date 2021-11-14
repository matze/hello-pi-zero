//! # Temperature connection
//!
//! DS18B20 -> Pi Zero
//!    Data -> 4
//!
//! # Display connection
//!
//! ```
//! Display -> Pi Zero
//!     VCC -> 3.3V
//!     GND -> GND
//!     CLK -> SCLK
//!     DIN -> MOSI
//!      CS -> 24 (BCM: CE0, 8)
//!     D/C -> 36 (BCM: 16)
//!     RES -> 35 (BCM: 19)
//!
//! ```
mod onewire;

use anyhow::{Context, Result};
use embedded_graphics as gfx;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use embedded_graphics::primitives::*;
use sh1106::displaysize::DisplaySize;
use sh1106::mode::GraphicsMode;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time;
use tokio::try_join;

/// Wrapper for tokio::time::sleep so we can use it in try_join!().
async fn fallible_sleep(duration: time::Duration) -> Result<()> {
    time::sleep(duration).await;
    Ok(())
}

struct InnerState {
    ds18b20: onewire::Ds18b20,
}

#[derive(Clone)]
struct State {
    inner: Arc<RwLock<InnerState>>,
}

impl State {
    /// Read the DS18B20 temperature.
    async fn home_temperature(&self) -> Result<f32> {
        Ok(self.inner.read().await.ds18b20.read().await?)
    }
}

#[rustfmt::skip]
const ARROW_UP: &[u8] = &[
    0b00000100, 0b0000_0000,
    0b00001110, 0b0000_0000,
    0b00011111, 0b0000_0000,
    0b00111111, 0b1000_0000,
    0b01111111, 0b1100_0000,
    0b11111111, 0b1110_0000,
];

#[rustfmt::skip]
const ARROW_DOWN: &[u8] = &[
    0b11111111, 0b1110_0000,
    0b01111111, 0b1100_0000,
    0b00111111, 0b1000_0000,
    0b00011111, 0b0000_0000,
    0b00001110, 0b0000_0000,
    0b00000100, 0b0000_0000,
];

const INPUT_DIGITS_HUGE: &[u8] = include_bytes!("assets/input-36-64.raw");
const INPUT_DIGITS_REGULAR: &[u8] = include_bytes!("assets/input-20-32.raw");
const DANGER: &[u8] = include_bytes!("assets/danger-24-24.raw");

struct Digits<'a> {
    atlas: gfx::image::ImageRaw<'a, BinaryColor>,
    sprite_size: Size,
    top_left: Point,
}

impl<'a> Digits<'a> {
    fn new(data: &'a [u8], top_left: Point, full_width: u32, sprite_size: Size) -> Self {
        Self {
            atlas: gfx::image::ImageRaw::<BinaryColor>::new(data, full_width),
            sprite_size,
            top_left,
        }
    }

    fn draw<DT>(&self, digits: u8, target: &mut DT) -> Result<(), DT::Error>
    where
        DT: DrawTarget<Color = BinaryColor>,
    {
        let digits = digits.clamp(0, 99);
        let d1 = (digits / 10) as i32;
        let d2 = (digits % 10) as i32;

        let size = &self.sprite_size;

        let d1 = self.atlas.sub_image(&Rectangle::new(
            Point::new(d1 * size.width as i32, 0),
            *size,
        ));

        let d2 = self.atlas.sub_image(&Rectangle::new(
            Point::new(d2 * size.width as i32, 0),
            *size,
        ));

        gfx::image::Image::new(&d1, self.top_left).draw(target)?;
        gfx::image::Image::new(
            &d2,
            self.top_left + Point::new(self.sprite_size.width as i32, 0),
        )
        .draw(target)?;

        Ok(())
    }
}

impl<'a> Dimensions for Digits<'a> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle {
            top_left: self.top_left,
            size: Size::new(self.sprite_size.width * 2, self.sprite_size.height),
        }
    }
}

struct ArrowIndicators<'a> {
    up: gfx::image::ImageRaw<'a, BinaryColor>,
    down: gfx::image::ImageRaw<'a, BinaryColor>,
    up_pos: Point,
    down_pos: Point,
}

impl<'a> ArrowIndicators<'a> {
    fn new<T: Dimensions>(thing: &T) -> Self {
        let bounding_box = thing.bounding_box();
        let x = bounding_box.top_left.x + bounding_box.size.width as i32 - 2;
        let y_up = bounding_box.top_left.y;
        let y_down = bounding_box.top_left.y + bounding_box.size.height as i32 - 11;

        Self {
            up: gfx::image::ImageRaw::<BinaryColor>::new(ARROW_UP, 11),
            down: gfx::image::ImageRaw::<BinaryColor>::new(ARROW_DOWN, 11),
            up_pos: Point::new(x, y_up),
            down_pos: Point::new(x, y_down),
        }
    }

    fn draw_up<DT>(&self, target: &mut DT) -> Result<(), DT::Error>
    where
        DT: DrawTarget<Color = BinaryColor>,
    {
        gfx::image::Image::new(&self.up, self.up_pos).draw(target)
    }

    fn draw_down<DT>(&self, target: &mut DT) -> Result<(), DT::Error>
    where
        DT: DrawTarget<Color = BinaryColor>,
    {
        gfx::image::Image::new(&self.down, self.down_pos).draw(target)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let spi = rppal::spi::Spi::new(
        rppal::spi::Bus::Spi0,
        rppal::spi::SlaveSelect::Ss0,
        400000,
        rppal::spi::Mode::Mode0,
    )
    .context("Unable to create SPI object")?;

    let gpio = rppal::gpio::Gpio::new()?;
    let cs = gpio.get(8)?.into_output();
    let dc = gpio.get(16)?.into_output();

    let mut display: GraphicsMode<_> = sh1106::builder::Builder::new()
        .with_size(DisplaySize::Display128x64)
        .connect_spi(spi, dc, cs)
        .into();

    display.init().unwrap();
    display.flush().unwrap();

    let inner = Arc::new(RwLock::new(InnerState {
        ds18b20: onewire::Ds18b20::new()?,
    }));

    let state = State { inner };

    let huge = Digits::new(INPUT_DIGITS_HUGE, Point::new(0, 0), 360, Size::new(36, 64));
    let small = Digits::new(INPUT_DIGITS_REGULAR, Point::new(88, 0), 200, Size::new(20, 32));
    let indicators = ArrowIndicators::new(&huge);
    let danger = gfx::image::ImageRaw::<BinaryColor>::new(DANGER, 24);

    let sleep_duration = time::Duration::from_millis(1000);

    let mut last_temperature = 20.0;

    loop {
        let (home_temperature, _) =
            try_join!(state.home_temperature(), fallible_sleep(sleep_duration))?;

        display.clear();

        let difference = home_temperature - last_temperature;
        last_temperature = home_temperature;

        huge.draw(home_temperature as u8, &mut display)?;

        if difference < -0.1 {
            indicators.draw_down(&mut display)?;
        }

        if difference > 0.1 {
            indicators.draw_up(&mut display)?;
        }

        small.draw(72, &mut display)?;

        gfx::image::Image::new(&danger, Point::new(100, 40)).draw(&mut display)?;

        display.flush().unwrap();
    }
}
