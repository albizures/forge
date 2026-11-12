use serde::{Deserialize, Serialize};

/// Request-local Workspace routing. `cwd` is supplied by the client, never the daemon.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecordSelection {
    pub workspace: Option<String>,
    pub cwd: Option<String>,
}

/// All Record operations carry the same explicit routing envelope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SelectedRecordRequest<T> {
    #[serde(flatten)]
    pub selection: RecordSelection,
    #[serde(flatten)]
    pub record: T,
}
// Serde's two flattened deserializers can discard fields before the inner
// request's deny_unknown_fields sees them. Extract routing explicitly so obsolete
// inputs cannot be silently ignored by the selected operator contract.
impl<'de, T: serde::de::DeserializeOwned> Deserialize<'de> for SelectedRecordRequest<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RequestFields;
        impl<'de> serde::de::Visitor<'de> for RequestFields {
            type Value = serde_json::Map<String, serde_json::Value>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an object with unique routing and Record request fields")
            }

            fn visit_map<M: serde::de::MapAccess<'de>>(
                self,
                mut map: M,
            ) -> Result<Self::Value, M::Error> {
                let mut fields = serde_json::Map::new();
                while let Some((key, value)) = map.next_entry::<String, serde_json::Value>()? {
                    if fields.contains_key(&key) {
                        return Err(serde::de::Error::custom(format!("duplicate field {key}")));
                    }
                    fields.insert(key, value);
                }
                Ok(fields)
            }
        }
        let mut fields = deserializer.deserialize_map(RequestFields)?;
        let selection = RecordSelection {
            workspace: serde_json::from_value(fields.remove("workspace").unwrap_or_default())
                .map_err(serde::de::Error::custom)?,
            cwd: serde_json::from_value(fields.remove("cwd").unwrap_or_default())
                .map_err(serde::de::Error::custom)?,
        };
        let record = serde_json::from_value(serde_json::Value::Object(fields))
            .map_err(serde::de::Error::custom)?;
        Ok(Self { selection, record })
    }
}

impl<T> SelectedRecordRequest<T> {
    pub fn selected(workspace: impl Into<String>, record: T) -> Self {
        Self {
            selection: RecordSelection {
                workspace: Some(workspace.into()),
                cwd: None,
            },
            record,
        }
    }
}
