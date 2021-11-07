use anyhow::{anyhow, Context, Result};
use std::path::{Path, PathBuf};
use tokio::fs::File;
use tokio::io::AsyncReadExt;

const DEVICE_PATH: &'static str = "/sys/bus/w1/devices";

pub struct Ds18b20 {
    path: PathBuf,
}

impl Ds18b20 {
    pub fn new() -> Result<Self> {
        let root = Path::new(DEVICE_PATH);

        for entry in root
            .read_dir()
            .with_context(|| format!("Cannot read {:?}", root))?
        {
            let entry = entry?;

            if entry
                .file_name()
                .to_str()
                .ok_or_else(|| anyhow!("Failed to convert {:?}", entry))?
                .starts_with("28-")
            // family code for DS18B20
            {
                let mut path = entry.path();
                path.push("temperature");

                if path.exists() && path.is_file() {
                    return Ok(Self { path });
                }
            }
        }

        Err(anyhow!("No DS18B20 device found"))
    }

    /// Open 1-wire device file and read the integer content as milli Celsius and convert back to
    /// normal degree Celsius.
    pub async fn read(&self) -> Result<f32> {
        let mut file = File::open(&self.path).await?;
        let mut content = String::new();
        file.read_to_string(&mut content).await?;
        Ok(content.trim().parse::<f32>()? / 1000.0)
    }
}
