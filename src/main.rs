mod met;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, FixedOffset};
use embedded_graphics as gfx;
use embedded_graphics::pixelcolor::BinaryColor;
use embedded_graphics::prelude::*;
use log::info;
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
    client: met::Client,
    expires: Option<DateTime<FixedOffset>>,
    last_response: Option<met::Response>,
}

#[derive(Clone)]
struct State {
    inner: Arc<RwLock<InnerState>>,
}

impl State {
    /// Return forecast data if not stale yet.
    async fn forecast(&self) -> Result<Vec<f32>> {
        let mut state = self.inner.write().await;
        let now = chrono::Local::now();

        // Return early if we should not update the forecast data
        if let Some(expires) = state.expires {
            if now < expires {
                if let Some(response) = &state.last_response {
                    return Ok(response.next_n_hours(now, 48)?);
                }
            }
        }

        info!("fetching forecast data");

        let response = state.client.get().await?;

        let value = response
            .headers()
            .get("expires")
            .ok_or_else(|| anyhow!("No expires in the header map"))?;

        let expires = chrono::DateTime::parse_from_rfc2822(value.to_str()?)?;
        state.expires = Some(expires);

        let response: met::Response = response.json().await?;
        let data = response.next_n_hours(now, 48)?;
        state.last_response = Some(response);

        Ok(data)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();

    let i2c = rppal::i2c::I2c::new().context("Unable to create I2c object")?;

    let mut display: GraphicsMode<_> = sh1106::builder::Builder::new()
        .with_size(DisplaySize::Display128x64)
        .connect_i2c(i2c)
        .into();

    display.init().unwrap();
    display.flush().unwrap();

    let inner = Arc::new(RwLock::new(InnerState {
        client: met::Client::new()?,
        expires: None,
        last_response: None,
    }));

    let state = State { inner };
    let text_style = gfx::mono_font::MonoTextStyle::new(&profont::PROFONT_7_POINT, BinaryColor::On);
    let line_style = gfx::primitives::PrimitiveStyleBuilder::new()
        .stroke_color(BinaryColor::On)
        .stroke_width(1)
        .build();
    let sleep_duration = time::Duration::from_millis(1500);

    let plot_x_start = 16;
    let scale_y_minimum = 36;
    let scale_y_maximum = 20;
    let scale_height = (scale_y_minimum - scale_y_maximum) as f32;

    loop {
        let (datapoints, _) = try_join!(state.forecast(), fallible_sleep(sleep_duration))?;

        let minimum = datapoints.iter().fold(f32::INFINITY, |a, &b| a.min(b));
        let maximum = datapoints.iter().fold(-f32::INFINITY, |a, &b| a.max(b));
        let range = maximum - minimum;
        let minimum_temp = format!("{:>2.0}°", minimum);
        let maximum_temp = format!("{:>2.0}°", maximum);

        // Would be great to update the display in a future as well but it's a pain to store it
        // in the `State` struct ...
        display.clear();

        // Draw scale mins and maxs
        gfx::text::Text::new(&maximum_temp, Point::new(0, scale_y_maximum), text_style)
            .draw(&mut display)?;
        gfx::text::Text::new(&minimum_temp, Point::new(0, scale_y_minimum), text_style)
            .draw(&mut display)?;

        // Draw pin plot
        for (index, temperature) in datapoints.iter().enumerate() {
            let x = (index * 2) as i32;
            let height = (((temperature - minimum) / range) * scale_height) as i32;
            let start = Point::new(plot_x_start + x, scale_y_minimum);
            let end = Point::new(plot_x_start + x, scale_y_minimum - height);
            gfx::primitives::Line::new(start, end)
                .into_styled(line_style)
                .draw(&mut display)?;
        }

        display.flush().unwrap();
    }
}
