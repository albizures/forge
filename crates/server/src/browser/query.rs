//! URL adaptation for ordinary GET forms and history-preserving links.
use forge_app::{
    RecordStatus, RecordType,
    record_browsing::{RecordFilters, RecordSort},
};

pub(super) const TYPES: [(&str, &str, RecordType); 6] = [
    ("generic", "Generic Record", RecordType::Generic),
    ("spec", "Spec", RecordType::Spec),
    ("task", "Task", RecordType::Task),
    ("wayfinder", "Wayfinder", RecordType::Wayfinder),
    ("grilling", "Grilling", RecordType::Grilling),
    ("initiative", "Initiative", RecordType::Initiative),
];
pub(super) const STATUSES: [(&str, &str, RecordStatus); 3] = [
    ("open", "Open", RecordStatus::Open),
    ("in_progress", "In Progress", RecordStatus::InProgress),
    ("done", "Done", RecordStatus::Done),
];
pub(super) const SORTS: [(&str, &str, RecordSort); 4] = [
    (
        "recently-updated",
        "Recently updated",
        RecordSort::RecentlyUpdated,
    ),
    (
        "least-recently-updated",
        "Least recently updated",
        RecordSort::LeastRecentlyUpdated,
    ),
    (
        "newest-created",
        "Newest created",
        RecordSort::NewestCreated,
    ),
    (
        "oldest-created",
        "Oldest created",
        RecordSort::OldestCreated,
    ),
];

pub(super) struct Choice {
    pub value: &'static str,
    pub label: &'static str,
    pub selected: bool,
}

pub(super) struct DashboardQuery {
    pub workspace: String,
    pub filters: RecordFilters,
    pub sort: RecordSort,
    pub page: usize,
}

impl DashboardQuery {
    pub fn parse(raw: &str) -> Result<Self, &'static str> {
        let mut query = Self {
            workspace: String::new(),
            filters: RecordFilters::default(),
            sort: RecordSort::default(),
            page: 1,
        };
        let (mut workspace_seen, mut sort_seen, mut page_seen) = (false, false, false);
        // Reject malformed escapes/UTF-8 rather than letting the form decoder replace them.
        let bytes = raw.as_bytes();
        for (index, byte) in bytes.iter().enumerate() {
            if *byte == b'%'
                && (index + 2 >= bytes.len()
                    || !bytes[index + 1].is_ascii_hexdigit()
                    || !bytes[index + 2].is_ascii_hexdigit())
            {
                return Err("Malformed URL encoding");
            }
        }
        for (key, value) in form_urlencoded::parse(bytes) {
            if key.contains('\u{fffd}') || value.contains('\u{fffd}') {
                return Err("Invalid URL text");
            }
            match key.as_ref() {
                "workspace" if !workspace_seen => {
                    query.workspace = value.into_owned();
                    workspace_seen = true;
                }
                "sort" if !sort_seen => {
                    query.sort = SORTS
                        .iter()
                        .find(|(wire, _, _)| *wire == value)
                        .ok_or("Unknown sort order")?
                        .2;
                    sort_seen = true;
                }
                "page" if !page_seen => {
                    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                        return Err("Page must be a positive integer");
                    }
                    query.page = value
                        .parse()
                        .ok()
                        .filter(|page| *page > 0)
                        .ok_or("Page must be a positive integer")?;
                    page_seen = true;
                }
                "type" => {
                    let kind = TYPES
                        .iter()
                        .find(|(wire, _, _)| *wire == value)
                        .ok_or("Unknown Record Type")?
                        .2;
                    if !query.filters.types.contains(&kind) {
                        query.filters.types.push(kind);
                    }
                }
                "status" => {
                    let status = STATUSES
                        .iter()
                        .find(|(wire, _, _)| *wire == value)
                        .ok_or("Unknown Record Status")?
                        .2;
                    if !query.filters.statuses.contains(&status) {
                        query.filters.statuses.push(status);
                    }
                }
                _ => return Err("Unknown or repeated query parameter"),
            }
        }
        if query.workspace.is_empty() && query.page != 1 {
            return Err("Select a Workspace before paging");
        }
        Ok(query)
    }

    pub fn url(&self, page: usize) -> String {
        let mut encoded = form_urlencoded::Serializer::new(String::new());
        encoded.append_pair("workspace", &self.workspace);
        for (wire, _, kind) in TYPES {
            if self.filters.types.contains(&kind) {
                encoded.append_pair("type", wire);
            }
        }
        for (wire, _, status) in STATUSES {
            if self.filters.statuses.contains(&status) {
                encoded.append_pair("status", wire);
            }
        }
        encoded.append_pair(
            "sort",
            SORTS
                .iter()
                .find(|(_, _, sort)| *sort == self.sort)
                .map_or("recently-updated", |(wire, _, _)| wire),
        );
        encoded.append_pair("page", &page.to_string());
        format!("/records?{}", encoded.finish())
    }

    pub fn sort_label(&self) -> &'static str {
        SORTS
            .iter()
            .find(|(_, _, sort)| *sort == self.sort)
            .map_or("Recently updated", |(_, label, _)| label)
    }
}
