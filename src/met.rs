use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Deserialize;

const FORECAST_URL: &'static str =
    "https://api.met.no/weatherapi/locationforecast/2.0/compact?lat=49.0068&lon=8.4036";

#[derive(Deserialize, Debug)]
pub struct Details {
    pub air_temperature: f32,
}

#[derive(Deserialize, Debug)]
pub struct Instant {
    pub details: Details,
}

#[derive(Deserialize, Debug)]
pub struct Data {
    pub instant: Instant,
}

#[derive(Deserialize, Debug)]
pub struct Timeseries {
    pub time: DateTime<Utc>,
    pub data: Data,
}

#[derive(Deserialize, Debug)]
pub struct Properties {
    pub timeseries: Vec<Timeseries>,
}

#[derive(Deserialize, Debug)]
pub struct Response {
    pub properties: Properties,
}

impl Response {
    pub fn next_n_hours(&self, dt: chrono::DateTime<chrono::Local>, n: usize) -> Result<Vec<f32>> {
        Ok(self
            .properties
            .timeseries
            .iter()
            .filter(|e| e.time > dt)
            .take(n)
            .map(|e| e.data.instant.details.air_temperature)
            .collect::<Vec<_>>())
    }
}

pub struct Client {
    client: reqwest::Client,
}

impl Client {
    pub fn new() -> Result<Self> {
        let client = reqwest::ClientBuilder::new()
            .user_agent("bloerg.net kontakt@bloerg.net")
            .build()?;
        Ok(Self { client })
    }

    pub async fn get(&self) -> Result<reqwest::Response> {
        Ok(self.client.get(FORECAST_URL).send().await?)
    }
}
