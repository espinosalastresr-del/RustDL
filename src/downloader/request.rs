//! Build HTTP client and requests.

use crate::config::Config;
use crate::errors::Result;
use reqwest::Client;
use std::time::Duration;

pub fn build_client(cfg: &Config) -> Result<Client> {
    let mut builder = Client::builder()
        .user_agent(&cfg.user_agent)
        .connect_timeout(cfg.connect_timeout())
        .redirect(reqwest::redirect::Policy::limited(
            cfg.max_redirects as usize,
        ));

    if let Some(secs) = cfg.timeouts.request_secs {
        if secs > 0 {
            builder = builder.timeout(Duration::from_secs(secs));
        }
    }

    if !cfg.verify_tls {
        builder = builder.danger_accept_invalid_certs(true);
    }

    Ok(builder.build()?)
}
