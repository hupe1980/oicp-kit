//! `PageQuery` — the `?page=&size=` pair the paginated pulls take.

use core::fmt;

use serde::{Deserialize, Serialize};

/// Which page of a paginated pull to fetch.
///
/// > *The parameters of the pagination are given at the end of the end point:
/// > `…?page=0&size=20` where `page` indicates the number of the page for the response and `size`
/// > the amount of records to be provided in the response.*
///
/// # Choosing a size
///
/// > *The default number of records provided in the response are **20** elements and the maximum
/// > number of records possible to obtain per page are **2000**.*
///
/// The default of 20 is the Spring Data default and far too small for a real crawl — a European
/// EVSE data set at 20 records per page is tens of thousands of round trips. So this crate asks
/// for [`MAX_SIZE`](Self::MAX_SIZE) records, which is both the documented maximum and the default
/// here, and the constructors clamp to it rather than sending a number Hubject will not honour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageQuery {
    /// The page number, counting from zero.
    pub page: u32,
    /// How many records to return.
    pub size: u32,
}

impl PageQuery {
    /// The largest page Hubject serves, and the page size this crate asks for.
    pub const MAX_SIZE: u32 = 2000;

    /// The first page, at [`MAX_SIZE`](Self::MAX_SIZE).
    #[must_use]
    pub const fn new() -> Self {
        Self { page: 0, size: Self::MAX_SIZE }
    }

    /// The first page, at `size` records — clamped to [`MAX_SIZE`](Self::MAX_SIZE).
    #[must_use]
    pub const fn with_size(size: u32) -> Self {
        Self::at(0, size)
    }

    /// A specific page, at `size` records — clamped to [`MAX_SIZE`](Self::MAX_SIZE).
    #[must_use]
    pub const fn at(page: u32, size: u32) -> Self {
        // A size above the documented maximum is not honoured, and a crawler that believes it
        // asked for 5000 and got 2000 walks the pages at the wrong stride.
        Self { page, size: if size > Self::MAX_SIZE { Self::MAX_SIZE } else { size } }
    }

    /// The next page, at the same size.
    #[must_use]
    pub const fn next(self) -> Self {
        Self { page: self.page + 1, size: self.size }
    }

    /// The query string, without the leading `?`.
    #[must_use]
    pub fn to_query_string(self) -> String {
        format!("page={}&size={}", self.page, self.size)
    }

    /// Appends this query to `url`, using `?` or `&` as appropriate.
    #[must_use]
    pub fn append_to(self, url: &str) -> String {
        let separator = if url.contains('?') { '&' } else { '?' };
        format!("{url}{separator}{}", self.to_query_string())
    }
}

impl Default for PageQuery {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for PageQuery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "page={}&size={}", self.page, self.size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_query_matches_the_form_the_spec_documents() {
        assert_eq!(PageQuery::at(0, 20).to_query_string(), "page=0&size=20");
    }

    #[test]
    fn a_crawl_walks_pages_at_a_fixed_size() {
        let first = PageQuery::with_size(500);
        assert_eq!(first.page, 0);
        let second = first.next();
        assert_eq!((second.page, second.size), (1, 500));
    }

    #[test]
    fn a_size_above_the_documented_maximum_is_clamped() {
        // "the maximum number of records possible to obtain per page are 2000". A crawler that
        // believes it asked for 5000 and silently got 2000 walks the pages at the wrong stride.
        assert_eq!(PageQuery::with_size(5000).size, PageQuery::MAX_SIZE);
        assert_eq!(PageQuery::at(3, 5000).size, PageQuery::MAX_SIZE);
        assert_eq!(PageQuery::with_size(500).size, 500);
        assert_eq!(PageQuery::new().size, PageQuery::MAX_SIZE);
    }

    #[test]
    fn appending_picks_the_right_separator() {
        assert_eq!(PageQuery::at(1, 20).append_to("https://x/y"), "https://x/y?page=1&size=20");
        assert_eq!(PageQuery::at(1, 20).append_to("https://x/y?a=b"), "https://x/y?a=b&page=1&size=20");
    }
}
