use std::collections::HashMap;

use anyhow::{anyhow, bail, Context, Result};
use quick_js::JsValue;
use serde_json::Value;
use tokio::task::spawn_blocking;
use tracing::instrument;

use crate::{
    composer::reporter::utils::get_oj_status, entities::SubmissionReportConfig,
    worker::run_container,
};

#[instrument(skip_all)]
pub async fn execute_javascript_reporter(
    data: Value,
    source: String,
) -> Result<SubmissionReportConfig> {
    spawn_blocking(move || run(data, source)).await?
}

fn run(data: Value, source: String) -> Result<SubmissionReportConfig> {
    let context = init_context(data).context("Error initializing the context")?;
    let source = format!("( function(DATA){{{source}}} )( DATA )");
    match context.eval(&source).context("Error executing the script")? {
        JsValue::Object(report) => Ok({
            serde_json::from_value(
                QuickJsObject(report).try_into().context("Error converting the returned object")?,
            )
            .context("Error deserializing the returned the object")?
        }),
        _ => bail!("Unknown return value by the reporter script"),
    }
}

fn init_context(data: Value) -> Result<quick_js::Context> {
    let context = quick_js::Context::new()?;

    context.set_global(
        "DATA",
        convert_to_js_value(data).context("Error converting the data into JsValue")?,
    )?;

    context.add_callback("getOJStatus", get_oj_status_wrapper)?;

    Ok(context)
}

fn get_oj_status_wrapper(
    run_report: HashMap<String, JsValue>,
    compare_report: HashMap<String, JsValue>,
) -> Result<&'static str> {
    use run_container::ExecutionReport;

    let run_report: ExecutionReport =
        serde_json::from_value(QuickJsObject(run_report).try_into()?)?;
    let compare_report: ExecutionReport =
        serde_json::from_value(QuickJsObject(compare_report).try_into()?)?;

    Ok(get_oj_status(run_report, compare_report).into())
}

struct QuickJsObject(HashMap<String, JsValue>);

impl TryInto<Value> for QuickJsObject {
    type Error = anyhow::Error;

    fn try_into(self) -> Result<Value> {
        convert_to_value(JsValue::Object(self.0))
    }
}

fn convert_to_value(value: JsValue) -> Result<Value> {
    use serde_json::Number;

    Ok(match value {
        JsValue::Undefined => Value::Null,
        JsValue::Null => Value::Null,
        JsValue::Bool(value) => Value::Bool(value),
        JsValue::Int(value) => Value::Number(Number::from(value)),
        JsValue::Float(value) => {
            Value::Number(Number::from_f64(value).ok_or_else(|| anyhow!("Invalid float number"))?)
        }
        JsValue::String(value) => Value::String(value),
        JsValue::Array(values) => {
            Value::Array(values.into_iter().map(convert_to_value).collect::<Result<_>>()?)
        }
        JsValue::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| convert_to_value(value).map(|value| (key, value)))
                .collect::<Result<_>>()?,
        ),
        _ => bail!("Unknown value detected"),
    })
}

fn convert_to_js_value(value: Value) -> Result<JsValue> {
    Ok(match value {
        Value::Null => JsValue::Null,
        Value::Bool(value) => JsValue::Bool(value),
        Value::String(value) => JsValue::String(value),
        Value::Number(value) => {
            if value.is_f64() {
                JsValue::Float(
                    value
                        .as_f64()
                        .ok_or_else(|| anyhow!("Error getting the value as f64: {value}"))?,
                )
            } else if value.is_i64() {
                JsValue::Int(
                    value
                        .as_i64()
                        .ok_or_else(|| anyhow!("Error getting the value as i64: {value}"))?
                        .try_into()
                        .with_context(|| format!("Error converting i64 into i32: {value}"))?,
                )
            } else {
                JsValue::Int(
                    value
                        .as_u64()
                        .ok_or_else(|| anyhow!("Error getting the value as u64: {value}"))?
                        .try_into()
                        .with_context(|| format!("Error converting u64 into i32: {value}"))?,
                )
            }
        }
        Value::Array(values) => {
            JsValue::Array(values.into_iter().map(convert_to_js_value).collect::<Result<_>>()?)
        }
        Value::Object(values) => JsValue::Object(
            values
                .into_iter()
                .map(|(key, value)| convert_to_js_value(value).map(|value| (key, value)))
                .collect::<Result<_>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use map_macro::hash_map;
    use quick_js::JsValue;

    #[tokio::test]
    async fn test_execute_javascript_reporter() {
        let data = serde_json::from_str(
            r#"{
            "submitted_at": "2023-01-28T12:48:09.155Z",
            "id": "complex",
            "steps": {
                "prepare": {
                    "status": "success",
                    "run_at": "2023-01-28T12:48:09.160Z",
                    "time_elapsed_ms": 0,
                    "report": null
                }
            }
        }"#,
        )
        .unwrap();
        let source = "return {report:{str:'foo',num:114,float_num:114.514,obj:{bool:true},arr:[1,\
                      1,4,5,1,4]}}"
            .to_string();

        let report = super::execute_javascript_reporter(data, source).await.unwrap();
        let json = serde_json::to_string(&report).unwrap();

        assert_eq!(json, r#"{"report":{"arr":[1,1,4,5,1,4],"float_num":114.514,"num":114,"obj":{"bool":true},"str":"foo"},"embeds":[],"uploads":[]}"#.to_string());
    }

    #[tokio::test]
    async fn test_convert_to_js_value() {
        let data = serde_json::from_str(
            r#"{
                "null": null,
                "bool": true,
                "string": "string",
                "integer": 114514,
                "float": 114.514,
                "array": [
                    "seele",
                    1,
                    true
                ],
                "object": {
                    "foo": "114",
                    "bar": 514
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            super::convert_to_js_value(data).unwrap(),
            JsValue::Object(hash_map! {
                "null".to_owned() => JsValue::Null,
                "bool".to_owned() => JsValue::Bool(true),
                "string".to_owned() => JsValue::String("string".to_owned()),
                "integer".to_owned() => JsValue::Int(114514),
                "float".to_owned() => JsValue::Float(114.514),
                "array".to_owned() => JsValue::Array(vec![JsValue::String("seele".to_owned()), JsValue::Int(1), JsValue::Bool(true)]),
                "object".to_owned() => JsValue::Object(hash_map! {
                    "foo".to_owned() => JsValue::String("114".to_owned()),
                    "bar".to_owned() => JsValue::Int(514),
                })
            })
        )
    }
}
