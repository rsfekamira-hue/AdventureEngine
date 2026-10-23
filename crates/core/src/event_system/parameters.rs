//! 共有ParameterSetの値、条件、更新。
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, PartialOrd)]
#[serde(untagged)]
pub enum ParameterValue {
    Bool(bool),
    Int(i32),
    String(String),
}
#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(transparent)]
pub struct ParameterSet {
    values: BTreeMap<String, ParameterValue>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Condition {
    pub parameter: String,
    pub op: String,
    pub value: ParameterValue,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub enum ParameterUpdate {
    Set {
        parameter: String,
        value: ParameterValue,
    },
    Add {
        parameter: String,
        value: i32,
    },
}
impl ParameterSet {
    pub fn get(&self, name: &str) -> Option<&ParameterValue> {
        self.values.get(name)
    }
    pub(super) fn validate_names(&self) -> Result<(), String> {
        if self.values.keys().any(String::is_empty) {
            return Err("パラメーター名が空です".into());
        }
        Ok(())
    }
    pub(super) fn validate_condition(&self, condition: &Condition) -> Result<(), String> {
        let actual = self
            .get(&condition.parameter)
            .ok_or_else(|| format!("条件のパラメーターがありません: {}", condition.parameter))?;
        if std::mem::discriminant(actual) != std::mem::discriminant(&condition.value)
            || !["eq", "ne", "gt", "gte", "lt", "lte"].contains(&condition.op.as_str())
            || (!["eq", "ne"].contains(&condition.op.as_str())
                && !matches!(actual, ParameterValue::Int(_)))
        {
            return Err("条件の型・比較が不正です".into());
        }
        Ok(())
    }
    /// 登録済みの名前と型を維持する。ロード時と公開更新入口で同じ規則を使う。
    pub(super) fn validate_update(&self, update: &ParameterUpdate) -> Result<(), String> {
        let (parameter, value_type) = match update {
            ParameterUpdate::Set { parameter, value } => (parameter, std::mem::discriminant(value)),
            ParameterUpdate::Add { parameter, .. } => {
                (parameter, std::mem::discriminant(&ParameterValue::Int(0)))
            }
        };
        let initial = self
            .get(parameter)
            .ok_or_else(|| format!("更新するパラメーターがありません: {parameter}"))?;
        if std::mem::discriminant(initial) != value_type {
            return Err("更新するパラメーターの型が違います".into());
        }
        Ok(())
    }
    /// 全更新を検証してから適用し、不正な更新では値を一切変更しない。
    pub fn apply(&mut self, updates: &[ParameterUpdate]) -> Result<(), String> {
        for update in updates {
            self.validate_update(update)?;
        }
        for update in updates {
            match update {
                ParameterUpdate::Set { parameter, value } => {
                    self.values.insert(parameter.clone(), value.clone());
                }
                ParameterUpdate::Add { parameter, value } => {
                    let Some(ParameterValue::Int(number)) = self.values.get_mut(parameter) else {
                        unreachable!()
                    };
                    *number = number.saturating_add(*value);
                }
            }
        }
        Ok(())
    }
    pub fn matches(&self, conditions: &[Condition]) -> bool {
        conditions.iter().all(|condition| {
            let actual = &self.values[&condition.parameter];
            match condition.op.as_str() {
                "eq" => actual == &condition.value,
                "ne" => actual != &condition.value,
                "gt" => actual > &condition.value,
                "gte" => actual >= &condition.value,
                "lt" => actual < &condition.value,
                "lte" => actual <= &condition.value,
                _ => unreachable!(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_updates_preserve_all_values_and_registered_types() {
        let mut parameters: ParameterSet =
            serde_json::from_str(r#"{"count":1,"ready":false}"#).unwrap();
        let before = serde_json::to_value(&parameters).unwrap();
        for invalid in [
            ParameterUpdate::Set {
                parameter: "missing".into(),
                value: ParameterValue::Int(1),
            },
            ParameterUpdate::Set {
                parameter: "count".into(),
                value: ParameterValue::Bool(true),
            },
            ParameterUpdate::Add {
                parameter: "ready".into(),
                value: 1,
            },
        ] {
            assert!(parameters
                .apply(&[
                    ParameterUpdate::Add {
                        parameter: "count".into(),
                        value: 2
                    },
                    invalid,
                ])
                .is_err());
            assert_eq!(serde_json::to_value(&parameters).unwrap(), before);
        }
    }

    #[test]
    fn integer_updates_saturate_at_both_limits_and_preserve_json_format() {
        let mut parameters: ParameterSet =
            serde_json::from_str(r#"{"upper":2147483647,"lower":-2147483648}"#).unwrap();
        parameters
            .apply(&[
                ParameterUpdate::Add {
                    parameter: "upper".into(),
                    value: 1,
                },
                ParameterUpdate::Add {
                    parameter: "lower".into(),
                    value: -1,
                },
            ])
            .unwrap();
        assert_eq!(
            parameters.get("upper"),
            Some(&ParameterValue::Int(i32::MAX))
        );
        assert_eq!(
            parameters.get("lower"),
            Some(&ParameterValue::Int(i32::MIN))
        );
        assert_eq!(
            serde_json::to_value(parameters).unwrap(),
            serde_json::json!({
                "upper": i32::MAX, "lower": i32::MIN,
            })
        );
    }
}
