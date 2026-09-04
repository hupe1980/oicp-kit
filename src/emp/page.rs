//! The Spring-Data page envelope every OICP pull comes back in.

use serde::{Deserialize, Serialize};

use crate::types::{Extensions, StatusCode, Validate, Validator, ViolationCode, validate_fields};

/// How a page's `pageable` block describes the sort applied.
///
/// Hubject's backend is Spring Data, and this block leaks through. Nothing in OICP depends on it,
/// so it is decoded and preserved rather than interpreted.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Sort {
    /// Whether the results are sorted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sorted: Option<bool>,
    /// Whether the sort is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<bool>,
    /// Whether the results are unsorted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsorted: Option<bool>,
}

/// The paging parameters the server echoes back.
#[derive(Clone, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Pageable {
    /// How the results were sorted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort: Option<Sort>,
    /// Items per page.
    #[serde(rename = "pageSize", default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    /// The page number.
    #[serde(rename = "pageNumber", default, skip_serializing_if = "Option::is_none")]
    pub page_number: Option<u32>,
    /// The offset of this page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Whether the response is paginated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paged: Option<bool>,
    /// Whether the response is unpaginated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unpaged: Option<bool>,
}

/// One page of a paginated OICP pull.
///
/// # Why pulls are paginated and pushes are not
///
/// A CPO pushes its own fleet; an EMP pulls **everyone's**. A single unfiltered `PullEvseData`
/// covers the charging points of every operator the EMP has a contract with across Europe —
/// hundreds of thousands of records. The spec's answer is `?page=0&size=20` on the query string,
/// with this envelope in the response.
///
/// The client crawls these for you: [`CrawlEvseData`](crate::client::EmpClient) yields records,
/// not pages, and never holds more than one page in memory. Use this type directly only when you
/// want the page metadata.
///
/// Spec: the response bodies of `eRoamingEVSEData` and `eRoamingChargeDetailRecords`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Page<T> {
    /// The records on this page.
    pub content: Vec<T>,
    /// The number of this page, counting from zero.
    pub number: u32,
    /// The page size that was asked for.
    pub size: u32,
    /// How many records the whole query matches.
    #[serde(rename = "totalElements")]
    pub total_elements: u64,
    /// How many pages the whole query covers.
    #[serde(rename = "totalPages")]
    pub total_pages: u32,
    /// Whether this is the first page.
    pub first: bool,
    /// Whether this is the last page.
    pub last: bool,
    /// How many records are on this page.
    #[serde(rename = "numberOfElements")]
    pub number_of_elements: u32,
    /// Whether the result set is empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<bool>,
    /// The paging parameters, echoed back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pageable: Option<Pageable>,
    /// Whether the query itself succeeded.
    #[serde(rename = "StatusCode", default, skip_serializing_if = "Option::is_none")]
    pub status_code: Option<StatusCode>,
    /// Undocumented fields, preserved verbatim.
    #[serde(flatten, default, skip_serializing_if = "Extensions::is_empty")]
    pub extensions: Extensions,
}

impl<T> Page<T> {
    /// The number of the next page, or `None` on the last one.
    ///
    /// Trusts [`last`](Self::last) over arithmetic on the counts: Hubject computes it, and on a
    /// data set that is changing under a crawl the two can disagree.
    #[must_use]
    pub fn next_page(&self) -> Option<u32> {
        if self.last { None } else { Some(self.number + 1) }
    }

    /// Whether the query itself failed, whatever the HTTP status was.
    #[must_use]
    pub fn is_error(&self) -> bool {
        self.status_code.as_ref().is_some_and(|s| !s.is_success())
    }

    /// The records, consuming the page.
    #[must_use]
    pub fn into_content(self) -> Vec<T> {
        self.content
    }

    /// Applies a fallible `f` to every record, keeping the page metadata.
    ///
    /// Stops at the first failure. Use it where a page has to arrive whole — and the crawls, where
    /// it does not, decode each record on its own instead.
    ///
    /// # Errors
    ///
    /// Whatever `f` returns.
    pub fn try_map<U, E>(self, mut f: impl FnMut(T) -> Result<U, E>) -> Result<Page<U>, E> {
        let mut content = Vec::with_capacity(self.content.len());
        for item in self.content {
            content.push(f(item)?);
        }
        Ok(Page {
            content,
            number: self.number,
            size: self.size,
            total_elements: self.total_elements,
            total_pages: self.total_pages,
            first: self.first,
            last: self.last,
            number_of_elements: self.number_of_elements,
            empty: self.empty,
            pageable: self.pageable,
            status_code: self.status_code,
            extensions: self.extensions,
        })
    }

    /// Applies `f` to every record, keeping the page metadata.
    pub fn map<U>(self, f: impl FnMut(T) -> U) -> Page<U> {
        Page {
            content: self.content.into_iter().map(f).collect(),
            number: self.number,
            size: self.size,
            total_elements: self.total_elements,
            total_pages: self.total_pages,
            first: self.first,
            last: self.last,
            number_of_elements: self.number_of_elements,
            empty: self.empty,
            pageable: self.pageable,
            status_code: self.status_code,
            extensions: self.extensions,
        }
    }
}

impl<T: Validate> Validate for Page<T> {
    fn validate_in(&self, v: &mut Validator) {
        // The counts have to agree with the content, or a crawler cannot tell whether it has
        // seen everything.
        if self.number_of_elements as usize != self.content.len() {
            v.report_at(
                "numberOfElements",
                ViolationCode::Inconsistent,
                format!(
                    "the page says {} records but carries {}",
                    self.number_of_elements,
                    self.content.len()
                ),
            );
        }
        if self.total_pages > 0 && self.number >= self.total_pages {
            v.report_at(
                "number",
                ViolationCode::OutOfRange,
                format!("this is page {} of {}", self.number, self.total_pages),
            );
        }
        if self.last != (self.total_pages == 0 || self.number + 1 >= self.total_pages) {
            v.report_at(
                "last",
                ViolationCode::Inconsistent,
                format!(
                    "last is {} on page {} of {}; a crawler that trusts it will stop early or loop",
                    self.last, self.number, self.total_pages
                ),
            );
        }
        if self.first != (self.number == 0) {
            v.report_at(
                "first",
                ViolationCode::Inconsistent,
                format!("first is {} on page {}", self.first, self.number),
            );
        }
        validate_fields!(self, v, content as "content", status_code as "StatusCode");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(number: u32, total_pages: u32, content: Vec<u32>) -> Page<u32> {
        Page {
            number_of_elements: u32::try_from(content.len()).unwrap(),
            content,
            number,
            size: 20,
            total_elements: u64::from(total_pages) * 20,
            total_pages,
            first: number == 0,
            last: number + 1 >= total_pages,
            empty: None,
            pageable: None,
            status_code: None,
            extensions: Extensions::new(),
        }
    }

    #[test]
    fn a_crawl_follows_next_page_until_last() {
        let mut visited = vec![];
        let mut n = Some(0);
        while let Some(current) = n {
            visited.push(current);
            n = page(current, 3, vec![1, 2]).next_page();
        }
        assert_eq!(visited, vec![0, 1, 2]);
    }

    #[test]
    fn page_counts_that_contradict_the_content_are_reported() {
        let mut p = page(0, 1, vec![1, 2]);
        assert!(p.validate().is_ok());

        p.number_of_elements = 5;
        assert_eq!(p.validate().unwrap_err().as_slice()[0].pointer, "/numberOfElements");
    }

    #[test]
    fn a_last_flag_that_would_truncate_a_crawl_is_reported() {
        let mut p = page(0, 3, vec![1]);
        p.last = true; // …on page 0 of 3: a crawler would stop after a third of the data.
        let err = p.validate().unwrap_err();
        assert_eq!(err.as_slice()[0].pointer, "/last");
    }

    #[test]
    fn an_empty_result_set_is_a_single_last_page() {
        let p = page(0, 0, vec![]);
        assert!(p.last);
        assert_eq!(p.next_page(), None);
        assert!(p.validate().is_ok());
    }
}
