// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! The worker's side of the `BotState` Durable Object (src/state.rs): get, put with an optional
//! expiry, delete. One instance holds everything, so a read sees the last write.
//!
//! Keys in use: `draft:<key>` (a review card's draft, 15 minutes), `tracked:<thread>` (the issue
//! a post became), `bug-rate:<user>` (one report a minute).

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use worker::wasm_bindgen::JsValue;
use worker::{Env, Error, Headers, Method, Request, RequestInit, Result, Stub};

pub struct Store {
    stub: Stub,
}

impl Store {
    pub fn new(env: &Env) -> Result<Self> {
        let stub = env
            .durable_object("STATE")?
            .id_from_name("bot")?
            .get_stub()?;
        Ok(Self { stub })
    }

    async fn call(&self, path: &str, body: &Value) -> Result<String> {
        let headers = Headers::new();
        headers.set("Content-Type", "application/json")?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(JsValue::from_str(&body.to_string())));
        let request = Request::new_with_init(&format!("https://state{path}"), &init)?;
        let mut response = self.stub.fetch_with_request(request).await?;
        if response.status_code() >= 300 {
            return Err(Error::RustError(format!(
                "state {path} answered {}",
                response.status_code()
            )));
        }
        response.text().await
    }

    pub async fn get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let text = self.call("/get", &json!({ "key": key })).await?;
        let value: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if value.is_null() {
            return Ok(None);
        }
        Ok(serde_json::from_value(value).ok())
    }

    pub async fn put<T: Serialize>(
        &self,
        key: &str,
        value: &T,
        ttl_seconds: Option<u64>,
    ) -> Result<()> {
        let value =
            serde_json::to_value(value).map_err(|error| Error::RustError(error.to_string()))?;
        self.call(
            "/put",
            &json!({ "key": key, "value": value, "ttl_seconds": ttl_seconds }),
        )
        .await
        .map(|_| ())
    }

    pub async fn delete(&self, key: &str) -> Result<()> {
        self.call("/delete", &json!({ "key": key }))
            .await
            .map(|_| ())
    }
}
