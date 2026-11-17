use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use super::search::map_reqwest_error;
use super::FreeToolError;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExchangeRateResult {
    pub base: String,
    pub target: String,
    pub rate: f64,
    pub amount: f64,
    pub converted: f64,
    pub date: String,
    pub source: String,
}

impl ExchangeRateResult {
    pub fn format_message(&self) -> String {
        format!(
            "💱 **{:.2} {} = {:.2} {}**\nTasa de referencia: 1 {} = {:.4} {} (fecha: {})\nFuente: {} *(tipo de cambio oficial de referencia; no refleja dólar blue, MEP ni cotizaciones minoristas)*",
            self.amount,
            self.base,
            self.converted,
            self.target,
            self.base,
            self.rate,
            self.target,
            self.date,
            self.source
        )
    }
}

#[derive(Deserialize)]
struct FrankfurterApiResponse {
    base: String,
    date: String,
    rates: HashMap<String, f64>,
}

pub struct FrankfurterProvider {
    pub client: Client,
    pub base_url: String,
    pub timeout: Duration,
}

impl FrankfurterProvider {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            base_url: "https://api.frankfurter.dev/v1/latest".to_string(),
            timeout: Duration::from_secs(10),
        }
    }

    #[cfg(test)]
    pub fn with_base_url(client: Client, base_url: impl Into<String>) -> Self {
        Self {
            client,
            base_url: base_url.into(),
            timeout: Duration::from_secs(5),
        }
    }

    pub async fn convert(
        &self,
        base: &str,
        target: &str,
        amount: f64,
    ) -> Result<ExchangeRateResult, FreeToolError> {
        let base_norm = base.trim().to_ascii_uppercase();
        let target_norm = target.trim().to_ascii_uppercase();

        if !is_valid_currency_code(&base_norm) || !is_valid_currency_code(&target_norm) {
            return Err(FreeToolError::InvalidRequest(
                "Indicá códigos de moneda ISO de 3 letras (ej: USD, EUR, BRL).".to_string(),
            ));
        }
        if !amount.is_finite() || amount <= 0.0 {
            return Err(FreeToolError::InvalidRequest(
                "El importe a convertir debe ser un número positivo.".to_string(),
            ));
        }

        if base_norm == target_norm {
            return Ok(ExchangeRateResult {
                base: base_norm,
                target: target_norm,
                rate: 1.0,
                amount,
                converted: amount,
                date: chrono::Utc::now().format("%Y-%m-%d").to_string(),
                source: "Referencia 1:1".to_string(),
            });
        }

        let resp = self
            .client
            .get(&self.base_url)
            .timeout(self.timeout)
            .query(&[
                ("base", base_norm.as_str()),
                ("symbols", target_norm.as_str()),
            ])
            .send()
            .await
            .map_err(map_reqwest_error)?;

        let status = resp.status();
        if status.as_u16() == 404 || status.as_u16() == 422 {
            return Err(FreeToolError::InvalidRequest(format!(
                "Frankfurter no ofrece cotización de referencia para {base_norm}/{target_norm} (no incluye dólar blue, MEP ni monedas no publicadas por el BCE)."
            )));
        }
        if !status.is_success() {
            return Err(FreeToolError::ProviderUnavailable(format!(
                "Frankfurter devolvió HTTP {}.",
                status.as_u16()
            )));
        }

        let body = resp.text().await.map_err(map_reqwest_error)?;
        parse_frankfurter_response(&body, &target_norm, amount)
    }
}

pub fn parse_frankfurter_response(
    body: &str,
    target: &str,
    amount: f64,
) -> Result<ExchangeRateResult, FreeToolError> {
    let parsed: FrankfurterApiResponse = serde_json::from_str(body).map_err(|_| {
        FreeToolError::ProviderUnavailable("Respuesta JSON inesperada de Frankfurter.".to_string())
    })?;
    let Some(&rate) = parsed.rates.get(target) else {
        return Err(FreeToolError::InvalidRequest(format!(
            "Frankfurter no devolvió cotización para la moneda '{target}'."
        )));
    };
    Ok(ExchangeRateResult {
        base: parsed.base,
        target: target.to_string(),
        rate,
        amount,
        converted: amount * rate,
        date: parsed.date,
        source: "Frankfurter (BCE)".to_string(),
    })
}

fn is_valid_currency_code(code: &str) -> bool {
    code.len() == 3 && code.bytes().all(|b| b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frankfurter_exchange_rate_and_rejects_missing_currency() {
        let json = r#"{
            "amount": 1.0,
            "base": "USD",
            "date": "2026-11-17",
            "rates": { "EUR": 0.92 }
        }"#;
        let res = parse_frankfurter_response(json, "EUR", 250.0).unwrap();
        assert_eq!(res.base, "USD");
        assert_eq!(res.target, "EUR");
        assert!((res.converted - 230.0).abs() < 1e-6);
        assert!(res.format_message().contains("no refleja dólar blue"));

        assert!(matches!(
            parse_frankfurter_response(json, "ARS", 100.0),
            Err(FreeToolError::InvalidRequest(_))
        ));
    }
}
