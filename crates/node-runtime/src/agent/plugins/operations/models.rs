//! Model operations reuse the existing typed installation and resource owners.
use super::*;
use sailry_protocol::installation::ModelRef;

pub(super) fn handles(operation: Operation) -> bool {
    matches!(operation, Operation::InstallModel | Operation::RemoveModel)
}

pub(super) fn description(operation: Operation) -> &'static str {
    match operation {
        Operation::InstallModel => "Install an admitted model on the execution Node",
        Operation::RemoveModel => "Uninstall an OCR model without removing its definition",
        _ => unreachable!(),
    }
}

pub(super) fn parameters() -> Value {
    json!({"type":"object","additionalProperties":false,"required":["model"],"properties":{"model":{"type":"object"}}})
}

pub(super) fn command(operation: Operation, arguments: Value) -> Result<Command, Fault> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        model: ModelRef,
    }
    let input: Input =
        serde_json::from_value(arguments).map_err(|_| Fault::invalid("invalid model arguments"))?;
    let command = input.model.node_command()?;
    match (operation, command) {
        (Operation::InstallModel, command) => Ok(command),
        (Operation::RemoveModel, Command::InstallOcrModel { id }) => {
            Ok(Command::DeleteOcrModel { id })
        }
        (Operation::RemoveModel, _) => Err(Fault::invalid("OCR model removal required")),
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_native_targets_and_definitions() {
        let arguments = json!({"model":{"target":"node","kind":"ocr","id":"a".repeat(64)}});
        assert!(
            matches!(command(Operation::InstallModel,arguments.clone()).unwrap(),Command::InstallOcrModel {id} if id=="a".repeat(64))
        );
        assert!(
            matches!(command(Operation::RemoveModel,arguments).unwrap(),Command::DeleteOcrModel {id} if id=="a".repeat(64))
        );
        for model in [
            json!({"target":"node","kind":"embedding","model":"granite-multi"}),
            json!({"target":"controller","kind":"speech","id":null}),
            json!({"target":"node","kind":"ocr","id":"invalid"}),
        ] {
            assert!(command(Operation::RemoveModel, json!({"model":model})).is_err());
        }
        for operation in [Operation::InstallModel, Operation::RemoveModel] {
            assert!(!operation.read_only());
            assert_eq!(operation.action(), Some(plugin::Action::InstallModels));
        }
    }
}
