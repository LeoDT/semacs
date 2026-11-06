use crate::{diff::DiffKind, mode::ModeId, status::Group};
use rquickjs::{Atom, Ctx, Object, Value, object::Filter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusSelection {
    pub group: Group,
    pub can_stage: bool,
    pub can_unstage: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkflowContext {
    pub mode: ModeId,
    pub selection: Option<StatusSelection>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkflowRequest {
    StatusOpen,
    StatusRefresh,
    DiffOpen(DiffKind),
    Stage,
    Unstage,
}

pub(super) fn parse(ctx: &Ctx<'_>, value: Value<'_>) -> Result<Option<WorkflowRequest>, String> {
    if value.is_null() {
        return Ok(None);
    }
    if value.is_array() || value.is_promise() || value.is_function() {
        return Err("Workflow must return null or one typed request".into());
    }
    let object = value
        .into_object()
        .ok_or("Workflow must return null or one typed request")?;
    parse_object(&object).map(Some).map_err(|error| {
        format!(
            "Invalid workflow request: {}",
            super::runtime::js_error(ctx, error)
        )
    })
}

fn short_string(object: &Object<'_>, key: &str, limit: usize) -> rquickjs::Result<String> {
    let value: rquickjs::String = object.get(key)?;
    if value.clone().to_cstring()?.len() > limit {
        return Err(rquickjs::Error::new_from_js_message(
            "string",
            "enum",
            "Workflow enum exceeds its limit",
        ));
    }
    value.to_string()
}

fn parse_object(object: &Object<'_>) -> rquickjs::Result<WorkflowRequest> {
    let invalid = || {
        rquickjs::Error::new_from_js_message(
            "object",
            "WorkflowRequest",
            "Expected an exact operation / comparison request",
        )
    };
    let operation_key = Atom::from_str(object.ctx().clone(), "operation")?;
    let comparison_key = Atom::from_str(object.ctx().clone(), "comparison")?;
    let mut operation_seen = false;
    let mut comparison_seen = false;
    for key in object.own_keys::<Atom>(Filter::new().string().symbol()) {
        let key = key?;
        if key == operation_key {
            operation_seen = true;
        } else if key == comparison_key {
            comparison_seen = true;
        } else {
            return Err(invalid());
        }
    }
    if !operation_seen {
        return Err(invalid());
    }
    let operation = short_string(object, "operation", 20)?;
    if operation == "magit.diff.open" {
        if !comparison_seen {
            return Err(invalid());
        }
        return match short_string(object, "comparison", 8)?.as_str() {
            "staged" => Ok(WorkflowRequest::DiffOpen(DiffKind::Staged)),
            "unstaged" => Ok(WorkflowRequest::DiffOpen(DiffKind::Unstaged)),
            _ => Err(invalid()),
        };
    }
    if comparison_seen {
        return Err(invalid());
    }
    match operation.as_str() {
        "magit.status.open" => Ok(WorkflowRequest::StatusOpen),
        "magit.status.refresh" => Ok(WorkflowRequest::StatusRefresh),
        "magit.stage" => Ok(WorkflowRequest::Stage),
        "magit.unstage" => Ok(WorkflowRequest::Unstage),
        _ => Err(invalid()),
    }
}
