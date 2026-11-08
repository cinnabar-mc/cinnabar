use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    UnsupportedOptionalExpression,
    UnreferencedDefinition,
    AmbiguousAnimationReference,
    /// The definition is malformed, so its animation is skipped.
    InvalidDefinition,
    /// The stack's animation clip, channel or keyframe budget is spent, so its animation is
    /// skipped instead of the whole stack.
    OverBudget,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    MissingRequiredReference,
    AmbiguousRequiredReference,
    MissingGeometryReference,
    AmbiguousGeometryReference,
    MissingRenderControllerReference,
    AmbiguousRenderControllerReference,
    AmbiguousAnimationReference,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "outcome", content = "detail")]
pub enum CompileReferenceOutcome<T> {
    Resolved(T),
    OptionalStaticFallback {
        source: u32,
        symbol: u32,
        reason: FallbackReason,
    },
    RequiredRigRejected {
        source: u32,
        symbol: u32,
        reason: RejectReason,
    },
}
