//! BTCPay Server API client.

use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument};

use crate::config::BTCPayConfig;

use super::error::{BTCPayError, Result};

/// Client for interacting with BTCPay Server's Greenfield API.
#[derive(Debug, Clone)]
pub struct BTCPayClient {
    http: Client,
    config: BTCPayConfig,
}

/// Request to create an invoice.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInvoiceRequest {
    /// Amount (as string for precision)
    pub amount: String,
    /// Currency (e.g., "SATS", "BTC", "USD")
    pub currency: String,
    /// Invoice metadata
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<InvoiceMetadata>,
    /// Checkout options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkout: Option<CheckoutOptions>,
}

/// Invoice metadata.
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceMetadata {
    /// Order ID for reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    /// Item description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item_desc: Option<String>,
}

/// Checkout options for invoice.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckoutOptions {
    /// URL to redirect after payment
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_url: Option<String>,
    /// Default payment method
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_payment_method: Option<String>,
}

/// Response from creating an invoice.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InvoiceResponse {
    /// Invoice ID
    pub id: String,
    /// Checkout link (URL to BTCPay checkout page)
    pub checkout_link: String,
    /// Invoice status
    pub status: String,
    /// Amount in the invoice currency
    pub amount: String,
    /// Currency
    pub currency: String,
}

impl BTCPayClient {
    /// Create a new BTCPay client.
    pub fn new(config: BTCPayConfig) -> Result<Self> {
        config
            .validate()
            .map_err(|e| BTCPayError::Config(e))?;

        let http = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self { http, config })
    }

    /// Get the webhook secret for signature verification.
    pub fn webhook_secret(&self) -> &str {
        &self.config.webhook_secret
    }

    /// Get the default tip amount in sats.
    pub fn default_tip_sats(&self) -> u64 {
        self.config.default_tip_sats
    }

    /// Get the BTCPay base URL (for modal script).
    pub fn base_url(&self) -> &str {
        &self.config.base_url
    }

    /// Create a new invoice.
    #[instrument(skip(self), fields(amount_sats))]
    pub async fn create_invoice(
        &self,
        amount_sats: u64,
        order_id: Option<String>,
        description: Option<String>,
        redirect_url: Option<String>,
    ) -> Result<InvoiceResponse> {
        let url = format!(
            "{}/api/v1/stores/{}/invoices",
            self.config.base_url, self.config.store_id
        );

        let request = CreateInvoiceRequest {
            amount: amount_sats.to_string(),
            currency: "SATS".to_string(),
            metadata: Some(InvoiceMetadata {
                order_id,
                item_desc: description,
            }),
            checkout: Some(CheckoutOptions {
                redirect_url,
                default_payment_method: Some("BTC-LightningNetwork".to_string()),
            }),
        };

        debug!(?request, "Creating BTCPay invoice");

        let response = self
            .http
            .post(&url)
            .header("Authorization", format!("token {}", self.config.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(BTCPayError::Api {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let invoice: InvoiceResponse = response
            .json()
            .await
            .map_err(|e| BTCPayError::InvalidResponse(e.to_string()))?;

        info!(
            invoice_id = %invoice.id,
            amount = %invoice.amount,
            "Invoice created successfully"
        );

        Ok(invoice)
    }

    /// Get invoice status.
    #[instrument(skip(self))]
    pub async fn get_invoice(&self, invoice_id: &str) -> Result<InvoiceResponse> {
        let url = format!(
            "{}/api/v1/stores/{}/invoices/{}",
            self.config.base_url, self.config.store_id, invoice_id
        );

        let response = self
            .http
            .get(&url)
            .header("Authorization", format!("token {}", self.config.api_key))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(BTCPayError::Api {
                status: status.as_u16(),
                message: error_text,
            });
        }

        let invoice: InvoiceResponse = response
            .json()
            .await
            .map_err(|e| BTCPayError::InvalidResponse(e.to_string()))?;

        Ok(invoice)
    }
}
