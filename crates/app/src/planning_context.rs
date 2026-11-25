//! Structured observations made by the initiating planning client.

/// An observation for one Project, not a commit pin or a working-tree snapshot.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PlanningContext {
    Available { repository: String, branch: String },
    Unavailable { reason: String },
}

/// Associates an observation with the immutable qualified Project reference.
#[derive(Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct ProjectPlanningContext {
    pub project: String,
    pub context: PlanningContext,
}

impl ProjectPlanningContext {
    /// Validate an observation before accepting it at a public boundary.
    ///
    /// # Errors
    /// Rejects unqualified Project identities and empty observation fields.
    pub fn validate(&self) -> Result<(), crate::RegistrationError> {
        crate::projects::ProjectReference::parse(&self.project)?;
        let valid = match &self.context {
            PlanningContext::Available { repository, branch } => {
                !repository.trim().is_empty() && !branch.trim().is_empty()
            }
            PlanningContext::Unavailable { reason } => !reason.trim().is_empty(),
        };
        if !valid {
            return Err(crate::RegistrationError::new(
                "invalid-planning-context",
                "planning context requires a repository and branch or a reason for unavailability",
            ));
        }
        Ok(())
    }
}

/// Validate that observations can only describe Projects in the Record's scope.
///
/// # Errors
/// Rejects invalid observations, duplicate Projects and out-of-scope Projects.
pub fn validate_scope(
    observations: &[ProjectPlanningContext],
    project_slugs: &[String],
) -> Result<(), crate::RegistrationError> {
    let mut seen = std::collections::HashSet::new();
    for observation in observations {
        observation.validate()?;
        if !project_slugs.contains(&observation.project) || !seen.insert(&observation.project) {
            return Err(crate::RegistrationError::new(
                "invalid-planning-context-scope",
                "planning context Projects must be unique and belong to the Record scope",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_must_be_unique_and_in_scope() {
        let observation = ProjectPlanningContext {
            project: "workspace/project".into(),
            context: PlanningContext::Unavailable {
                reason: "no checkout".into(),
            },
        };
        let scope = vec!["workspace/project".into()];
        assert!(validate_scope(std::slice::from_ref(&observation), &scope).is_ok());
        assert!(validate_scope(&[observation.clone(), observation.clone()], &scope).is_err());
        assert!(validate_scope(&[observation], &["workspace/other".into()]).is_err());
        assert!(validate_scope(&[], &scope).is_ok());
    }

    #[test]
    fn validates_project_and_observation_fields() {
        let available = ProjectPlanningContext {
            project: "workspace/project".into(),
            context: PlanningContext::Available {
                repository: "/checkout".into(),
                branch: "feature".into(),
            },
        };
        assert!(available.validate().is_ok());
        let unavailable = ProjectPlanningContext {
            context: PlanningContext::Unavailable {
                reason: "detached HEAD".into(),
            },
            ..available.clone()
        };
        assert!(unavailable.validate().is_ok());
        assert!(
            ProjectPlanningContext {
                project: "project".into(),
                ..available.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            ProjectPlanningContext {
                context: PlanningContext::Available {
                    repository: " ".into(),
                    branch: "main".into()
                },
                ..available.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            ProjectPlanningContext {
                context: PlanningContext::Available {
                    repository: "/checkout".into(),
                    branch: String::new()
                },
                ..available.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            ProjectPlanningContext {
                context: PlanningContext::Unavailable { reason: " ".into() },
                ..available
            }
            .validate()
            .is_err()
        );
    }
}
