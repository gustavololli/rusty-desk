use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Mutex as AsyncMutex;

pub const FORECAST_INTERVAL: Duration = Duration::from_secs(3600);
const AIR_INTERVAL: Duration = Duration::from_secs(3 * 3600);
const RETRY_INTERVAL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeatherCondition {
    Daylight,
    Night,
    Clear,
    Cloudy,
    Storm,
    FrozenPrecipitation,
    Rain,
    RainSoon,
    Wind,
    Dust,
    Heat,
    Cold,
    Fog,
}

impl WeatherCondition {
    pub fn message(self) -> &'static str {
        match self {
            Self::Daylight | Self::Night | Self::Clear | Self::Cloudy => "",
            Self::Storm => "El modelo indica tormenta en la zona.",
            Self::FrozenPrecipitation => "El modelo indica nieve o precipitación helada.",
            Self::Rain => "El modelo indica lluvia ahora.",
            Self::RainSoon => "Hay chances de lluvia en las próximas horas.",
            Self::Wind => "El modelo indica viento fuerte o ráfagas.",
            Self::Dust => "El modelo estima más polvo en el aire.",
            Self::Heat => "Subió la sensación térmica.",
            Self::Cold => "Bajó la sensación térmica.",
            Self::Fog => "El modelo indica niebla.",
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
struct HourlyWeather {
    time: Vec<String>,
    temperature_2m: Vec<Option<f64>>,
    apparent_temperature: Vec<Option<f64>>,
    precipitation_probability: Vec<Option<u64>>,
    rain: Vec<Option<f64>>,
    showers: Vec<Option<f64>>,
    weather_code: Vec<Option<u64>>,
    wind_gusts_10m: Vec<Option<f64>>,
    #[serde(default)]
    is_day: Vec<Option<u8>>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
struct AirQuality {
    current: Option<AirSample>,
    hourly: AirHours,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
struct AirSample {
    dust: Option<f64>,
    pm10: Option<f64>,
    pm2_5: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
struct AirHours {
    time: Vec<String>,
    dust: Vec<Option<f64>>,
}

#[derive(Default)]
struct CachedLocation {
    place: Option<GeocodingPlace>,
    forecast: Option<(DateTime<Utc>, WeatherReport)>,
    air: Option<(DateTime<Utc>, AirQuality)>,
    air_attempt: Option<DateTime<Utc>>,
    error: Option<(DateTime<Utc>, String)>,
}

#[derive(Default)]
pub struct WeatherCache {
    locations: Mutex<HashMap<String, Arc<AsyncMutex<CachedLocation>>>>,
}

fn recent(at: DateTime<Utc>, max_age: Duration) -> bool {
    (Utc::now() - at).to_std().is_ok_and(|age| age < max_age)
}

impl WeatherCache {
    pub async fn get(&self, client: &reqwest::Client, city: &str) -> Result<WeatherReport, String> {
        let location = {
            let mut locations = self.locations.lock().map_err(|e| e.to_string())?;
            Arc::clone(locations.entry(city.to_lowercase()).or_default())
        };
        let mut cached = location.lock().await;
        let forecast = if let Some((_, error)) = cached
            .error
            .as_ref()
            .filter(|(at, _)| recent(*at, RETRY_INTERVAL))
        {
            Err(error.clone())
        } else if let Some((_, forecast)) = cached
            .forecast
            .as_ref()
            .filter(|(at, _)| recent(*at, FORECAST_INTERVAL))
        {
            Ok(forecast.clone())
        } else {
            if cached.place.is_none() {
                match geocode(client, city).await {
                    Ok(place) => cached.place = Some(place),
                    Err(error) => {
                        cached.error = Some((Utc::now(), error.clone()));
                        return Err(error);
                    }
                }
            }
            match forecast_for(client, cached.place.as_ref().unwrap()).await {
                Ok(report) => {
                    cached.error = None;
                    cached.forecast = Some((Utc::now(), report.clone()));
                    Ok(report)
                }
                Err(error) => {
                    cached.error = Some((Utc::now(), error.clone()));
                    Err(error)
                }
            }
        };

        let mut report = match forecast {
            Ok(report) => report,
            Err(error) => {
                let Some((_, mut previous)) = cached.forecast.clone() else {
                    return Err(error);
                };
                previous.stale = true;
                previous.air = cached
                    .air
                    .as_ref()
                    .filter(|(at, _)| recent(*at, AIR_INTERVAL))
                    .map(|(_, air)| air.clone());
                return Ok(previous);
            }
        };
        if !cached
            .air_attempt
            .is_some_and(|at| recent(at, AIR_INTERVAL))
        {
            cached.air_attempt = Some(Utc::now());
            if let Ok(air) = fetch_air(client, cached.place.as_ref().unwrap()).await {
                cached.air = Some((Utc::now(), air));
            }
        }
        report.air = cached
            .air
            .as_ref()
            .filter(|(at, _)| recent(*at, AIR_INTERVAL))
            .map(|(_, air)| air.clone());
        Ok(report)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherReport {
    pub location_label: String,
    pub temperature_c: f64,
    pub apparent_temp_c: Option<f64>,
    pub humidity_pct: Option<u64>,
    pub wind_kmh: f64,
    pub weather_code: u64,
    pub fetched_at: DateTime<Utc>,
    pub stale: bool,
    pub gust_kmh: Option<f64>,
    pub current_time: String,
    pub is_day: Option<u8>,
    timezone: String,
    hourly: Option<HourlyWeather>,
    air: Option<AirQuality>,
    pub temp_max_c: Option<f64>,
    pub temp_min_c: Option<f64>,
}

pub struct WeatherHour<'a> {
    pub time: &'a str,
    pub temperature_c: Option<f64>,
    pub weather_code: Option<u64>,
    pub precipitation_pct: Option<u64>,
    pub is_day: Option<u8>,
}

impl WeatherReport {
    pub fn upcoming_hours(&self) -> impl Iterator<Item = WeatherHour<'_>> {
        self.hourly.iter().flat_map(|hourly| {
            let first = hourly
                .time
                .partition_point(|time| time <= &self.current_time);
            (first..hourly.time.len()).map(|i| WeatherHour {
                time: &hourly.time[i],
                temperature_c: hourly.temperature_2m.get(i).copied().flatten(),
                weather_code: hourly.weather_code.get(i).copied().flatten(),
                precipitation_pct: hourly.precipitation_probability.get(i).copied().flatten(),
                is_day: hourly.is_day.get(i).copied().flatten(),
            })
        })
    }

    pub fn air_measurements(&self) -> impl Iterator<Item = (&'static str, f64)> + '_ {
        self.air
            .iter()
            .filter_map(|air| air.current.as_ref())
            .flat_map(|air| {
                [
                    ("Polvo", air.dust),
                    ("PM10", air.pm10),
                    ("PM2.5", air.pm2_5),
                ]
                .into_iter()
                .filter_map(|(label, value)| value.map(|value| (label, value)))
            })
    }

    pub fn format_message(&self, hours: usize) -> String {
        let (icon, desc) = describe_wmo_code(self.weather_code);
        let mut details = vec![format!("{icon} **{}**: {desc}", self.location_label)];
        let mut temp_line = format!("🌡️ {:.1}°C", self.temperature_c);
        if let Some(app) = self.apparent_temp_c {
            temp_line.push_str(&format!(" (sensación {:.1}°C)", app));
        }
        if let (Some(min), Some(max)) = (self.temp_min_c, self.temp_max_c) {
            temp_line.push_str(&format!(" · Mín {:.1}°C / Máx {:.1}°C", min, max));
        }
        details.push(temp_line);

        let mut extra = vec![format!("💨 Viento: {:.0} km/h", self.wind_kmh)];
        if let Some(hum) = self.humidity_pct {
            extra.push(format!("💧 Humedad: {hum}%"));
        }
        if let Some(gust) = self.gust_kmh {
            extra.push(format!("Ráfagas: {gust:.0} km/h"));
        }
        details.push(extra.join(" · "));
        if let Some(air) = self.air.as_ref().and_then(|air| air.current.as_ref()) {
            let values = [
                ("Polvo", air.dust),
                ("PM10", air.pm10),
                ("PM2.5", air.pm2_5),
            ];
            let values: Vec<_> = values
                .into_iter()
                .filter_map(|(name, value)| value.map(|value| format!("{name} {value:.1} µg/m³")))
                .collect();
            if !values.is_empty() {
                details.push(format!("Aire estimado por CAMS: {}", values.join(" · ")));
            }
        }
        if let Some(hourly) = &self.hourly {
            let first = hourly
                .time
                .partition_point(|time| time <= &self.current_time);
            if hours > 0 && first < hourly.time.len() {
                details.push(format!("Pronóstico por hora ({}):", self.timezone));
            }
            for index in (first..hourly.time.len()).take(hours) {
                let mut values = Vec::new();
                if let Some(code) = hourly.weather_code[index] {
                    values.push(describe_wmo_code(code).1.to_owned());
                }
                if let Some(t) = hourly.temperature_2m[index] {
                    values.push(format!("{t:.0} °C"));
                }
                if let Some(t) = hourly.apparent_temperature[index] {
                    values.push(format!("sensación {t:.0} °C"));
                }
                if let Some(p) = hourly.precipitation_probability[index] {
                    values.push(format!("precipitación {p}%"));
                }
                if let Some(g) = hourly.wind_gusts_10m[index] {
                    values.push(format!("ráfagas {g:.0} km/h"));
                }
                if let Some(air) = &self.air {
                    if let Ok(i) = air.hourly.time.binary_search(&hourly.time[index]) {
                        if let Some(dust) = air.hourly.dust.get(i).copied().flatten() {
                            values.push(format!("polvo {dust:.1} µg/m³"));
                        }
                    }
                }
                details.push(format!(
                    "{} · {}",
                    hourly.time[index].replace('T', " "),
                    values.join(" · ")
                ));
            }
        }
        if self.stale {
            details.push("⚠️ Último pronóstico guardado. No se pudo actualizar.".into());
        }
        details.push(format!(
            "Consultado: {} UTC · Open-Meteo",
            self.fetched_at.format("%Y-%m-%d %H:%M")
        ));
        details.join("\n")
    }

    pub fn sky(&self) -> (Option<u8>, Option<bool>) {
        let cloudy = match self.weather_code {
            0 | 1 => Some(false),
            2 | 3 => Some(true),
            _ => None,
        };
        (self.is_day, cloudy)
    }

    pub fn condition(
        &self,
        previous: Option<WeatherCondition>,
        dust_limit: Option<f64>,
    ) -> Option<WeatherCondition> {
        use WeatherCondition::*;
        let code = self.weather_code;
        let hours = self.hourly.as_ref();
        let future = hours
            .map(|h| {
                let start = h.time.partition_point(|time| time <= &self.current_time);
                start..(start + 3).min(h.time.len())
            })
            .unwrap_or(0..0);
        let storm = |code| matches!(code, 95..=97 | 99);
        if storm(code)
            || hours.is_some_and(|h| future.clone().any(|i| h.weather_code[i].is_some_and(storm)))
        {
            return Some(Storm);
        }
        if matches!(code, 56 | 57 | 66 | 67 | 71 | 73 | 75 | 77 | 85 | 86) {
            return Some(FrozenPrecipitation);
        }
        if matches!(code, 51 | 53 | 55 | 61 | 63 | 65 | 80..=82) {
            return Some(Rain);
        }
        if hours.is_some_and(|h| {
            future.clone().any(|i| {
                h.precipitation_probability[i]
                    .is_some_and(|p| p >= if previous == Some(RainSoon) { 50 } else { 60 })
                    && (h.rain[i].unwrap_or(0.0) + h.showers[i].unwrap_or(0.0) > 0.0)
            })
        }) {
            return Some(RainSoon);
        }
        let (wind, gust) = if previous == Some(Wind) {
            (27.0, 40.0)
        } else {
            (30.0, 45.0)
        };
        if self.wind_kmh >= wind || self.gust_kmh.is_some_and(|g| g >= gust) {
            return Some(Wind);
        }
        if self
            .air
            .as_ref()
            .zip(dust_limit.filter(|x| *x > 0.0))
            .is_some_and(|(air, limit)| {
                let start = air
                    .hourly
                    .time
                    .partition_point(|time| time < &self.current_time);
                [start, start + 3].into_iter().all(|i| {
                    air.hourly
                        .dust
                        .get(i)
                        .copied()
                        .flatten()
                        .is_some_and(|dust| dust >= limit)
                })
            })
        {
            return Some(Dust);
        }
        let feels = self.apparent_temp_c.unwrap_or(self.temperature_c);
        if feels >= if previous == Some(Heat) { 30.0 } else { 32.0 } {
            return Some(Heat);
        }
        if feels <= if previous == Some(Cold) { 10.0 } else { 8.0 } {
            return Some(Cold);
        }
        matches!(code, 45 | 48).then_some(Fog)
    }
}

pub fn describe_wmo_code(code: u64) -> (&'static str, &'static str) {
    match code {
        0 => ("☀️", "Despejado"),
        1 => ("🌤️", "Mayormente despejado"),
        2 => ("⛅", "Parcialmente nublado"),
        3 => ("☁️", "Nublado"),
        45 | 48 => ("🌫️", "Niebla"),
        51 | 53 | 55 => ("🌦️", "Llovizna"),
        56 | 57 => ("🌧️", "Llovizna helada"),
        61 | 63 | 65 => ("🌧️", "Lluvia"),
        66 | 67 => ("🌧️", "Lluvia helada"),
        71 | 73 | 75 | 77 => ("❄️", "Nieve"),
        80..=82 => ("🌦️", "Chaparrones"),
        85 | 86 => ("🌨️", "Chaparrones de nieve"),
        95 | 97 => ("⛈️", "Tormenta eléctrica"),
        96 | 99 => ("⛈️", "Tormenta con granizo"),
        _ => ("🌡️", "Condición variable"),
    }
}

#[derive(Debug, Deserialize)]
struct GeocodingResponse {
    results: Option<Vec<GeocodingPlace>>,
}

#[derive(Debug, Deserialize)]
struct GeocodingPlace {
    name: String,
    latitude: f64,
    longitude: f64,
    country: Option<String>,
    admin1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ForecastResponse {
    current: CurrentWeather,
    daily: Option<DailyWeather>,
    hourly: Option<HourlyWeather>,
    #[serde(default)]
    timezone: String,
}

#[derive(Debug, Deserialize)]
struct CurrentWeather {
    is_day: Option<u8>,
    #[serde(default)]
    time: String,
    wind_gusts_10m: Option<f64>,
    temperature_2m: f64,
    relative_humidity_2m: Option<u64>,
    apparent_temperature: Option<f64>,
    weather_code: u64,
    wind_speed_10m: f64,
}

#[derive(Debug, Deserialize)]
struct DailyWeather {
    temperature_2m_max: Option<Vec<Option<f64>>>,
    temperature_2m_min: Option<Vec<Option<f64>>>,
}

fn build_location_label(place: &GeocodingPlace) -> String {
    let mut parts = vec![place.name.clone()];
    if let Some(admin) = place
        .admin1
        .as_deref()
        .filter(|s| !s.is_empty() && *s != place.name)
    {
        parts.push(admin.to_string());
    }
    if let Some(country) = place.country.as_deref().filter(|s| !s.is_empty()) {
        parts.push(country.to_string());
    }
    parts.join(", ")
}

fn parse_forecast(location_label: String, forecast: ForecastResponse) -> WeatherReport {
    let (temp_max_c, temp_min_c) = forecast
        .daily
        .map(|d| {
            (
                d.temperature_2m_max
                    .and_then(|v| v.into_iter().next().flatten()),
                d.temperature_2m_min
                    .and_then(|v| v.into_iter().next().flatten()),
            )
        })
        .unwrap_or((None, None));

    WeatherReport {
        location_label,
        temperature_c: forecast.current.temperature_2m,
        apparent_temp_c: forecast.current.apparent_temperature,
        humidity_pct: forecast.current.relative_humidity_2m,
        wind_kmh: forecast.current.wind_speed_10m,
        weather_code: forecast.current.weather_code,
        fetched_at: Utc::now(),
        stale: false,
        gust_kmh: forecast.current.wind_gusts_10m,
        current_time: forecast.current.time,
        is_day: forecast.current.is_day,
        timezone: forecast.timezone,
        hourly: forecast.hourly.filter(|h| {
            let len = h.time.len();
            [
                h.temperature_2m.len(),
                h.apparent_temperature.len(),
                h.precipitation_probability.len(),
                h.rain.len(),
                h.showers.len(),
                h.weather_code.len(),
                h.wind_gusts_10m.len(),
            ]
            .into_iter()
            .all(|size| size == len)
        }),
        air: None,
        temp_max_c,
        temp_min_c,
    }
}

async fn geocode(client: &reqwest::Client, query: &str) -> Result<GeocodingPlace, String> {
    let geo: GeocodingResponse = client
        .get("https://geocoding-api.open-meteo.com/v1/search")
        .query(&[
            ("name", query),
            ("count", "1"),
            ("language", "es"),
            ("format", "json"),
        ])
        .send()
        .await
        .map_err(|e| format!("Error consultando geocoding: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Error HTTP en geocoding: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Respuesta inválida de geocoding: {e}"))?;

    geo.results
        .and_then(|results| results.into_iter().next())
        .ok_or_else(|| format!("No encontré la ciudad '{query}'."))
}

async fn forecast_for(
    client: &reqwest::Client,
    place: &GeocodingPlace,
) -> Result<WeatherReport, String> {
    let lat = place.latitude.to_string();
    let lon = place.longitude.to_string();
    let label = build_location_label(place);

    let forecast: ForecastResponse = client
        .get("https://api.open-meteo.com/v1/forecast")
        .query(&[
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            (
                "current",
                "temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m,wind_gusts_10m,is_day",
            ),
            ("hourly", "temperature_2m,apparent_temperature,precipitation_probability,rain,showers,weather_code,wind_gusts_10m,is_day"),
            ("daily", "temperature_2m_max,temperature_2m_min"),
            ("timezone", "auto"),
            ("forecast_days", "2"),
        ])
        .send()
        .await
        .map_err(|e| format!("Error consultando el clima: {e}"))?
        .error_for_status()
        .map_err(|e| format!("Error HTTP del clima: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Respuesta inválida del clima: {e}"))?;

    Ok(parse_forecast(label, forecast))
}

async fn fetch_air(
    client: &reqwest::Client,
    place: &GeocodingPlace,
) -> Result<AirQuality, reqwest::Error> {
    client
        .get("https://air-quality-api.open-meteo.com/v1/air-quality")
        .query(&[
            ("latitude", place.latitude.to_string().as_str()),
            ("longitude", place.longitude.to_string().as_str()),
            ("current", "dust,pm10,pm2_5"),
            ("hourly", "dust"),
            ("timezone", "auto"),
            ("forecast_days", "2"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_meteo_payload_and_formats_report() {
        let geo: GeocodingResponse = serde_json::from_str(
            r#"{
                "results": [{
                    "name": "Buenos Aires",
                    "latitude": -34.61315,
                    "longitude": -58.37723,
                    "country": "Argentina",
                    "admin1": "Ciudad Autónoma de Buenos Aires"
                }]
            }"#,
        )
        .unwrap();
        let place = geo.results.unwrap().remove(0);
        let label = build_location_label(&place);
        assert_eq!(
            label,
            "Buenos Aires, Ciudad Autónoma de Buenos Aires, Argentina"
        );

        let forecast: ForecastResponse = serde_json::from_str(
            r#"{
                "current": {
                    "temperature_2m": 22.4,
                    "relative_humidity_2m": 58,
                    "apparent_temperature": 21.9,
                    "weather_code": 2,
                    "wind_speed_10m": 14.2
                },
                "daily": {
                    "temperature_2m_max": [26.0],
                    "temperature_2m_min": [15.5]
                }
            }"#,
        )
        .unwrap();

        let report = parse_forecast(label, forecast);
        let msg = report.format_message(6);
        assert!(msg.contains("Buenos Aires"));
        assert!(msg.contains("Parcialmente nublado"));
        assert!(msg.contains("22.4°C"));
        assert!(msg.contains("58%"));
    }

    #[test]
    fn classifies_forecast_and_distinguishes_dust_from_particulate_matter() {
        let forecast: ForecastResponse = serde_json::from_value(serde_json::json!({
            "timezone": "America/Argentina/Cordoba",
            "current": {
                "time": "2026-11-26T14:30",
                "temperature_2m": 30.0,
                "apparent_temperature": 31.0,
                "weather_code": 2,
                "wind_speed_10m": 20.0,
                "wind_gusts_10m": 44.0
            },
            "hourly": {
                "time": ["2026-11-26T14:00", "2026-11-26T15:00", "2026-11-26T16:00"],
                "temperature_2m": [30.0, 30.0, 28.0],
                "apparent_temperature": [31.0, 31.0, 28.0],
                "precipitation_probability": [null, 80, 70],
                "rain": [null, 0.0, 1.0],
                "showers": [null, 0.0, null],
                "weather_code": [2, 2, 61],
                "wind_gusts_10m": [44.0, 44.0, null]
            }
        }))
        .unwrap();
        let mut report = parse_forecast("Córdoba".into(), forecast);
        assert_eq!(
            report.condition(None, None),
            Some(WeatherCondition::RainSoon)
        );
        let message = report.format_message(2);
        assert!(message.contains("2026-11-26 16:00 · Lluvia"));
        assert!(!message.contains("2026-11-26 14:00"));
        report.hourly = None;
        assert_eq!(report.condition(None, None), None);
        assert_eq!(
            report.condition(Some(WeatherCondition::Wind), None),
            Some(WeatherCondition::Wind)
        );
        assert_eq!(
            report.condition(Some(WeatherCondition::Heat), None),
            Some(WeatherCondition::Heat)
        );
        report.air = Some(serde_json::from_value(serde_json::json!({
            "current": {"dust": 20, "pm10": 150},
            "hourly": {
                "time": ["2026-11-26T15:00", "2026-11-26T16:00", "2026-11-26T17:00", "2026-11-26T18:00"],
                "dust": [32, 31, 30, 29]
            }
        })).unwrap());
        assert_eq!(report.condition(None, Some(30.0)), None);
        assert_eq!(
            report.condition(None, Some(25.0)),
            Some(WeatherCondition::Dust)
        );
        assert_eq!(report.condition(None, Some(0.0)), None);
        assert!(report.format_message(0).contains("PM10 150.0"));
    }

    #[test]
    fn wmo_codes_map_to_stable_spanish_labels() {
        for (code, desc) in [
            (0, "Despejado"),
            (3, "Nublado"),
            (45, "Niebla"),
            (55, "Llovizna"),
            (61, "Lluvia"),
            (71, "Nieve"),
            (80, "Chaparrones"),
            (95, "Tormenta eléctrica"),
            (96, "Tormenta con granizo"),
            (999, "Condición variable"),
        ] {
            assert_eq!(describe_wmo_code(code).1, desc);
        }
    }

    #[test]
    fn location_label_skips_a_repeated_admin_area() {
        let place = GeocodingPlace {
            name: "Córdoba".into(),
            latitude: 0.0,
            longitude: 0.0,
            country: Some("Argentina".into()),
            admin1: Some("Córdoba".into()),
        };
        assert_eq!(build_location_label(&place), "Córdoba, Argentina");
        let bare = GeocodingPlace {
            name: "Ushuaia".into(),
            latitude: 0.0,
            longitude: 0.0,
            country: Some(String::new()),
            admin1: None,
        };
        assert_eq!(build_location_label(&bare), "Ushuaia");
    }
}
