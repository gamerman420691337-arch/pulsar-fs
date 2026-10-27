//! Preserve host-declared required-property order in native model grammars.
//! Schema values and protocol decoding remain independent of output key order.
use serde::{ser::SerializeMap, ser::SerializeSeq, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

pub(super) fn encode(schema: &Value) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&OrderedSchema(schema))
}

struct OrderedSchema<'a>(&'a Value);
impl Serialize for OrderedSchema<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Object(fields) => {
                let mut map = serializer.serialize_map(Some(fields.len()))?;
                for (name, value) in fields {
                    if name == "properties" {
                        let properties = value.as_object().ok_or_else(|| {
                            serde::ser::Error::custom("schema properties must be an object")
                        })?;
                        let required = match fields.get("required") {
                            Some(value) => value
                                .as_array()
                                .ok_or_else(|| {
                                    serde::ser::Error::custom("schema required must be an array")
                                })?
                                .as_slice(),
                            None => &[],
                        };
                        map.serialize_entry(
                            name,
                            &OrderedProperties {
                                properties,
                                required,
                            },
                        )?;
                    } else {
                        map.serialize_entry(name, &OrderedSchema(value))?;
                    }
                }
                map.end()
            }
            Value::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    seq.serialize_element(&OrderedSchema(value))?;
                }
                seq.end()
            }
            value => value.serialize(serializer),
        }
    }
}

struct OrderedProperties<'a> {
    properties: &'a Map<String, Value>,
    required: &'a [Value],
}
impl Serialize for OrderedProperties<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.properties.len()))?;
        let mut seen = BTreeSet::new();
        for entry in self.required {
            let name = entry.as_str().ok_or_else(|| {
                serde::ser::Error::custom("required property name must be a string")
            })?;
            if !seen.insert(name) {
                return Err(serde::ser::Error::custom("duplicate required property"));
            }
            let value = self
                .properties
                .get(name)
                .ok_or_else(|| serde::ser::Error::custom("required property is undeclared"))?;
            map.serialize_entry(name, &OrderedSchema(value))?;
        }
        for (name, value) in self.properties {
            if !seen.contains(name.as_str()) {
                map.serialize_entry(name, &OrderedSchema(value))?;
            }
        }
        map.end()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_discriminators_precede_parameters_without_changing_schema_values() {
        let schema = serde_json::json!({"oneOf":[{"type":"object","properties":{
            "patterns":{"type":"array","items":{"oneOf":[{"properties":{
                "amplitude":{"const":0.0},"pattern":{"const":"hold"},"axis":{"enum":["stroke"]}
            },"required":["pattern","axis","amplitude"]}]}},"kind":{"const":"synthesize_patterns"}
        },"required":["kind","patterns"]}]});
        let bytes = encode(&schema).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), schema);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.find("\"kind\":").unwrap() < text.find("\"patterns\":").unwrap());
        assert!(text.find("\"pattern\":").unwrap() < text.find("\"axis\":").unwrap());
        assert!(text.find("\"axis\":").unwrap() < text.find("\"amplitude\":").unwrap());
    }

    #[test]
    fn legacy_schema_bytes_and_optional_properties_are_preserved() {
        let legacy = serde_json::json!({"oneOf":[{"properties":{"kind":{"const":"select_target"},"target_id":{"enum":["cup2"]}},"required":["kind","target_id"],"additionalProperties":false}]});
        assert_eq!(
            encode(&legacy).unwrap(),
            serde_json::to_vec(&legacy).unwrap()
        );
        let optional = serde_json::json!({"properties":{"alpha":{"type":"string"},"zeta":{"type":"number"}},"required":["zeta"]});
        let bytes = encode(&optional).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), optional);
        let text = String::from_utf8(bytes).unwrap();
        assert!(text.find("\"zeta\":").unwrap() < text.find("\"alpha\":").unwrap());
        let no_required = serde_json::json!({"properties":{"optional":{"type":"string"}}});
        assert_eq!(
            encode(&no_required).unwrap(),
            serde_json::to_vec(&no_required).unwrap()
        );
    }

    #[test]
    fn malformed_schema_metadata_refuses_instead_of_emitting_duplicate_keys() {
        for schema in [
            serde_json::json!({"properties":[],"required":[]}),
            serde_json::json!({"properties":{},"required":false}),
            serde_json::json!({"properties":{"x":{}},"required":[1]}),
            serde_json::json!({"properties":{"x":{}},"required":["x","x"]}),
            serde_json::json!({"properties":{"x":{}},"required":["missing"]}),
        ] {
            assert!(encode(&schema).is_err());
        }
    }
}
