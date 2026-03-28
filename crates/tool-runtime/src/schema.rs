//! JSON Schema validation for tool arguments.

use serde_json::Value;

use crate::ToolError;

/// Validate `instance` against a JSON Schema value (default meta-schema inference).
pub fn validate_instance(schema: &Value, instance: &Value) -> Result<(), ToolError> {
    jsonschema::validate(schema, instance).map_err(|e| ToolError::Validation(e.to_string()))
}
