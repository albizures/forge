//! Project-owned ordered criteria, copied rather than linked into new Tasks.
use crate::{Application, RegistrationError};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckCriterion {
    pub name: String,
    pub description: String,
    /// Immutable Task Agent Profile revision; absent for profile-less Manual work.
    pub profile_revision: Option<u32>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckTemplate {
    pub version: u64,
    pub criteria: Vec<CheckCriterion>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TemplateChange {
    pub id: u64,
    pub version: u64,
    pub criteria: Vec<CheckCriterion>,
    pub actor: String,
    pub message: String,
    pub created_at_millis: u64,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TemplatePage {
    pub version: u64,
    pub criteria: Vec<CheckCriterion>,
    pub history: Vec<TemplateChange>,
    pub next_cursor: Option<u64>,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TemplateReplacement {
    pub expected_version: u64,
    pub criteria: Vec<CheckCriterion>,
    pub actor: String,
    pub message: String,
}
/// Default required baseline, also used for Projectless Tasks.
#[must_use]
pub fn baseline() -> Vec<CheckCriterion> {
    vec![CheckCriterion {
        name: "Implementation".into(),
        description: crate::checks::IMPLEMENTATION_CRITERION.into(),
        profile_revision: None,
    }]
}

impl TemplateReplacement {
    /// Validate and normalize the entire replacement without side effects.
    ///
    /// # Errors
    /// Rejects blank attribution, explanations or criteria, duplicate/reserved names.
    pub fn normalized_criteria(&self) -> Result<Vec<CheckCriterion>, RegistrationError> {
        if self.actor.trim().is_empty() || self.message.trim().is_empty() {
            return Err(RegistrationError::new(
                "invalid-check-template",
                "actor and explanation must be nonblank",
            ));
        }
        if self
            .criteria
            .first()
            .is_none_or(|c| c.name.trim() != "Implementation")
        {
            return Err(RegistrationError::new(
                "invalid-check-template",
                "first criterion must be Implementation",
            ));
        }
        let mut names = HashSet::new();
        self.criteria.iter().map(|criterion| {
            let name = criterion.name.trim();
            let description = criterion.description.trim();
            let key = name.to_lowercase();
            if name.is_empty() || description.is_empty() || criterion.profile_revision == Some(0) || !names.insert(key) {
                return Err(RegistrationError::new("invalid-check-template", "criteria require nonblank descriptions and unique nonblank names; Implementation must be first"));
            }
            Ok(CheckCriterion { name: name.into(), description: description.into(), profile_revision: criterion.profile_revision })
        }).collect()
    }
}
impl Application {
    /// Read the current template and one bounded ascending change-history page.
    ///
    /// # Errors
    /// Returns scope, pagination or storage errors.
    pub fn show_check_template(
        &self,
        project: &str,
        after: Option<u64>,
        limit: u32,
    ) -> Result<TemplatePage, RegistrationError> {
        self.records.show_check_template(project, after, limit)
    }
    /// Atomically replace future Task criteria at the observed template version.
    ///
    /// # Errors
    /// Rejects invalid criteria, inaccessible Projects or stale versions.
    pub fn replace_check_template(
        &self,
        project: &str,
        input: &TemplateReplacement,
    ) -> Result<CheckTemplate, RegistrationError> {
        input.normalized_criteria()?;
        self.records.replace_check_template(project, input)
    }
}
