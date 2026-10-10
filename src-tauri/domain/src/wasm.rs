use crate::{contracts::*, recurrence, ServiceError};
use wasm_bindgen::prelude::*;
fn failure(error: ServiceError) -> JsValue {
    JsValue::from_str(&serde_json::to_string(&error).unwrap_or_else(|_| error.message))
}
fn parse<T: serde::de::DeserializeOwned>(input: &str) -> Result<T, JsValue> {
    serde_json::from_str(input).map_err(|e| failure(crate::error("validation", &e.to_string())))
}
fn output<T: serde::Serialize>(value: T) -> Result<String, JsValue> {
    serde_json::to_string(&value).map_err(|e| failure(crate::error("service", &e.to_string())))
}
#[wasm_bindgen]
pub fn expand(series: &str, range: &str) -> Result<String, JsValue> {
    output(recurrence::expand(&parse(series)?, &parse(range)?).map_err(failure)?)
}
#[wasm_bindgen]
pub fn preview(event: &str) -> Result<String, JsValue> {
    output(recurrence::preview(parse(event)?).map_err(failure)?)
}
#[wasm_bindgen]
pub fn plan_mutation(series: &str, request: &str, new_id: &str) -> Result<String, JsValue> {
    output(recurrence::plan_mutation(&parse(series)?, &parse(request)?, new_id).map_err(failure)?)
}
#[wasm_bindgen]
pub fn expectation(series: &str) -> Result<String, JsValue> {
    recurrence::expectation(&parse(series)?).map_err(failure)
}
#[wasm_bindgen]
pub fn get_occurrence(series: &str, key: &str) -> Result<String, JsValue> {
    output(recurrence::get_occurrence(&parse(series)?, key).map_err(failure)?)
}
#[wasm_bindgen]
pub fn validate(event: &str) -> Result<String, JsValue> {
    output(crate::validate_event(parse::<CalendarEvent>(event)?).map_err(failure)?)
}
