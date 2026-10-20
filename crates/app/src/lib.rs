//! Application-owned operations and their result types.

/// The application boundary for operations shared by all transports.
#[derive(Debug, Default)]
pub struct Application;

impl Application {
    /// Creates the ready initial application instance.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Returns readiness information for this application instance.
    #[must_use]
    pub fn status(&self) -> ApplicationStatus {
        ApplicationStatus {
            name: "forge",
            version: env!("CARGO_PKG_VERSION"),
            ready: true,
        }
    }
}

/// Application-owned readiness information.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApplicationStatus {
    /// The application name.
    pub name: &'static str,
    /// The application version.
    pub version: &'static str,
    /// Whether the daemon/application is ready to serve requests.
    pub ready: bool,
}

#[cfg(test)]
mod tests {
    use super::Application;

    #[test]
    fn status_reports_the_ready_application_identity() {
        let status = Application::new().status();

        assert_eq!(status.name, "forge");
        assert_eq!(status.version, "0.1.0");
        assert!(status.ready);
    }
}
