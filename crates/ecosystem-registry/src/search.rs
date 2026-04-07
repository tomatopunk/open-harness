//! Search functionality for ecosystem registry

use crate::types::{ComponentMetadata, ComponentType, SearchResult};

/// Search query parameters
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    /// Search text
    pub query: Option<String>,
    /// Filter by component type
    pub component_type: Option<ComponentType>,
    /// Filter by tags
    pub tags: Vec<String>,
    /// Filter by author
    pub author: Option<String>,
    /// Page number (1-indexed)
    pub page: usize,
    /// Items per page
    pub per_page: usize,
    /// Sort by field
    pub sort_by: SortField,
    /// Sort order
    pub sort_order: SortOrder,
}

/// Sort field
#[derive(Debug, Clone, Default)]
pub enum SortField {
    #[default]
    Relevance,
    Downloads,
    Created,
    Updated,
    Name,
}

/// Sort order
#[derive(Debug, Clone, Default)]
pub enum SortOrder {
    Ascending,
    #[default]
    Descending,
}

/// Filter and sort search results
pub fn process_search_results(
    results: Vec<ComponentMetadata>,
    query: &SearchQuery,
) -> SearchResult {
    let mut filtered = results;

    // Filter by component type
    if let Some(ty) = &query.component_type {
        filtered.retain(|c| &c.component_type == ty);
    }

    // Filter by tags
    if !query.tags.is_empty() {
        filtered.retain(|c| query.tags.iter().all(|tag| c.tags.contains(tag)));
    }

    // Filter by author
    if let Some(author) = &query.author {
        filtered.retain(|c| c.authors.iter().any(|a| a.contains(author)));
    }

    // Filter by search text
    if let Some(q) = &query.query {
        let q_lower = q.to_lowercase();
        filtered.retain(|c| {
            c.name.to_lowercase().contains(&q_lower)
                || c.display_name.to_lowercase().contains(&q_lower)
                || c.description.to_lowercase().contains(&q_lower)
                || c.tags.iter().any(|t| t.to_lowercase().contains(&q_lower))
        });
    }

    // Sort
    sort_results(&mut filtered, query);

    // Pagination
    let total = filtered.len();
    let offset = (query.page - 1) * query.per_page;
    let end = (offset + query.per_page).min(total);

    let items = if offset >= total { Vec::new() } else { filtered[offset..end].to_vec() };

    SearchResult { total, items, page: query.page, per_page: query.per_page }
}

fn sort_results(results: &mut [ComponentMetadata], query: &SearchQuery) {
    results.sort_by(|a, b| {
        let ord = match query.sort_by {
            SortField::Relevance => {
                // For relevance, we rely on the order from the server
                std::cmp::Ordering::Equal
            }
            SortField::Downloads => a.downloads.cmp(&b.downloads),
            SortField::Created => {
                let a_time = a.created_at.map(|dt| dt.and_utc().timestamp()).unwrap_or(0i64);
                let b_time = b.created_at.map(|dt| dt.and_utc().timestamp()).unwrap_or(0i64);
                a_time.cmp(&b_time)
            }
            SortField::Updated => {
                let a_time = a.updated_at.map(|dt| dt.and_utc().timestamp()).unwrap_or(0i64);
                let b_time = b.updated_at.map(|dt| dt.and_utc().timestamp()).unwrap_or(0i64);
                a_time.cmp(&b_time)
            }
            SortField::Name => a.name.cmp(&b.name),
        };

        match query.sort_order {
            SortOrder::Ascending => ord,
            SortOrder::Descending => ord.reverse(),
        }
    });
}
