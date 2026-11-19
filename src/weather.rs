use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub struct WeatherReport {
    pub location_label: String,
    pub temperature_c: f64,
    pub apparent_temp_c: Option<f64>,
    pub humidity_pct: Option<u64>,
    pub wind_kmh: f64,
    pub weather_code: u64,
    pub temp_max_c: Option<f64>,
    pub temp_min_c: Option<f64>,
}

impl WeatherReport {
    pub fn format_message(&self) -> String {
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
        details.push(extra.join(" · "));
        details.join("\n")
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
        95 => ("⛈️", "Tormenta eléctrica"),
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
}

#[derive(Debug, Deserialize)]
struct CurrentWeather {
    temperature_2m: f64,
    relative_humidity_2m: Option<u64>,
    apparent_temperature: Option<f64>,
    weather_code: u64,
    wind_speed_10m: f64,
}

#[derive(Debug, Deserialize)]
struct DailyWeather {
    temperature_2m_max: Option<Vec<f64>>,
    temperature_2m_min: Option<Vec<f64>>,
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
                d.temperature_2m_max.and_then(|v| v.into_iter().next()),
                d.temperature_2m_min.and_then(|v| v.into_iter().next()),
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
        temp_max_c,
        temp_min_c,
    }
}

pub async fn fetch_weather(city: &str) -> Result<WeatherReport, String> {
    let query = city.trim();
    if query.is_empty() {
        return Err("Indicá una ciudad (ej: `/clima Buenos Aires`).".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| format!("No se pudo inicializar cliente HTTP: {e}"))?;

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

    let place = geo
        .results
        .and_then(|mut r| (!r.is_empty()).then(|| r.remove(0)))
        .ok_or_else(|| format!("No encontré la ciudad '{query}'."))?;

    let lat = place.latitude.to_string();
    let lon = place.longitude.to_string();
    let label = build_location_label(&place);

    let forecast: ForecastResponse = client
        .get("https://api.open-meteo.com/v1/forecast")
        .query(&[
            ("latitude", lat.as_str()),
            ("longitude", lon.as_str()),
            (
                "current",
                "temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m",
            ),
            ("daily", "temperature_2m_max,temperature_2m_min"),
            ("timezone", "auto"),
            ("forecast_days", "1"),
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

pub fn fetch_weather_blocking(city: &str) -> Result<WeatherReport, String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("No se pudo iniciar runtime HTTP: {e}"))?;
    rt.block_on(fetch_weather(city))
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
        let msg = report.format_message();
        assert!(msg.contains("Buenos Aires"));
        assert!(msg.contains("Parcialmente nublado"));
        assert!(msg.contains("22.4°C"));
        assert!(msg.contains("58%"));
    }

    #[test]
    fn wmo_codes_map_to_stable_spanish_labels_and_empty_city_does_not_fetch() {
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
        let err = fetch_weather_blocking("   ").unwrap_err();
        assert!(err.contains("ciudad"), "{err}");
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
