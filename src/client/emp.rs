//! `EmpClient` — the pulls, and the streaming crawl that makes them usable.

use futures_core::Stream;
use futures_util::StreamExt as _;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;

use super::http::{ClientConfig, Transport, warn_on_identity_mismatch};
use super::identity::{ClientIdentity, IdentityWarning};
use crate::cpo::{AuthorizeRemoteStartRequest, AuthorizeRemoteStopRequest, ChargeDetailRecord};
use crate::emp::{
    ChargeDetailRecordsResponse, EvsePricingResponse, EvseStatusByIdResponse, EvseStatusResponse,
    GetChargeDetailRecordsRequest, Page, PricingProductDataResponse, PullEvseDataRecord, PullEvseDataRequest,
    PullEvsePricingRequest, PullEvseStatusByIdRequest, PullEvseStatusByOperatorIdRequest,
    PullEvseStatusRequest, PullPricingProductDataRequest, PushAuthenticationDataRequest,
};
use crate::transport::{HubjectEnv, OicpError, Operation, PageQuery, PathId, Result};
use crate::types::{Acknowledgement, ProviderId, Validate};

/// A record that could not be decoded, with enough context to find it.
///
/// A crawl yields `Result<PullEvseDataRecord, CrawlError>`, so one operator's malformed record does
/// not cost the other 1999 on the page.
#[derive(Debug, thiserror::Error)]
pub enum CrawlError {
    /// A page could not be fetched at all.
    #[error("page {page} could not be fetched: {source}")]
    Page {
        /// Which page.
        page: u32,
        /// Why.
        #[source]
        source: OicpError,
    },
    /// One record on a page could not be decoded.
    ///
    /// The crawl continues: one operator's malformed record costs that record and no other.
    #[error("record {index} on page {page} could not be decoded: {message}")]
    Record {
        /// Which page.
        page: u32,
        /// Which record on it.
        index: usize,
        /// Why.
        message: String,
    },
    /// A page's own paging fields contradict each other.
    ///
    /// Yielded, not fatal — and the crawl keeps going by `totalPages` rather than believing a
    /// `last` flag that would end it early. A crawler that trusts `last` on page 0 of 300 stores
    /// a third of a percent of Europe and reports success.
    #[error("page {page} is inconsistent: {message}")]
    PageInconsistent {
        /// Which page.
        page: u32,
        /// What contradicts what.
        message: String,
    },
}

/// Decodes one page's records individually, and works out whether to ask for another.
///
/// This is the whole reason a page is fetched as [`RawValue`]s: decoding
/// `Page<PullEvseDataRecord>` in one step makes a single malformed record fail the **page**, and
/// a failed page ends the crawl. Envelope first, then each record on its own, is the difference
/// between losing one charging point and losing every charging point after it.
///
/// # Following the pages
///
/// A page says twice whether there is more — once in `last`, once in `totalPages` — and the two
/// can disagree. Neither is trusted alone:
///
/// * `last` wrong in one direction ends a crawl after a third of Europe, silently, with every
///   count reading as a success.
/// * `last` wrong in the other, or a server that never advances `number`, is an endless crawl.
///
/// So the walk is **bounded by `totalPages` whenever it is known**, `last` ends it when it is not,
/// an empty page always ends it, and every disagreement is yielded as a
/// [`PageInconsistent`](CrawlError::PageInconsistent) rather than resolved in silence.
fn decode_page<T: DeserializeOwned>(
    page: Page<Box<RawValue>>,
    query: PageQuery,
) -> (Vec<core::result::Result<T, CrawlError>>, Option<PageQuery>) {
    let mut items: Vec<core::result::Result<T, CrawlError>> = Vec::with_capacity(page.content.len() + 1);

    let counted = page.total_pages > 0;
    let more_by_count = counted && query.page + 1 < page.total_pages;
    let more_by_flag = !page.last;

    let (next, complaint) = if page.content.is_empty() {
        // Nothing here. Whatever the flags say, asking for the same nothing again is worse.
        (None, more_by_flag.then(|| format!("page {} is empty but last is false", page.number)))
    } else if more_by_flag && (more_by_count || !counted) {
        (Some(query.next()), None)
    } else if more_by_flag {
        // The flag wants another page, the count says this was the last: the count wins, because
        // it is the one that terminates.
        (None, Some(format!("last is false on page {} of {}", page.number, page.total_pages)))
    } else if more_by_count {
        // The flag would end the crawl here and the count says two thirds of the data is still to
        // come. Ending would look exactly like success.
        (
            Some(query.next()),
            Some(format!(
                "last is true on page {} of {}; crawling on by totalPages rather than stopping",
                page.number, page.total_pages
            )),
        )
    } else {
        (None, None)
    };

    if let Some(message) = complaint {
        items.push(Err(CrawlError::PageInconsistent { page: query.page, message }));
    }
    for (index, raw) in page.content.into_iter().enumerate() {
        items.push(match serde_json::from_str::<T>(raw.get()) {
            Ok(record) => Ok(record),
            Err(error) => Err(CrawlError::Record { page: query.page, index, message: error.to_string() }),
        });
    }
    (items, next)
}

/// The EMP's client: pulls, remote control, CDR retrieval.
///
/// Build it with [`EmpClient::builder`].
#[derive(Debug, Clone)]
pub struct EmpClient {
    transport: Transport,
    provider_id: ProviderId,
    identity_warning: Option<IdentityWarning>,
}

/// Builds an [`EmpClient`].
#[derive(Default)]
pub struct EmpClientBuilder {
    environment: Option<HubjectEnv>,
    provider_id: Option<ProviderId>,
    identity: Option<ClientIdentity>,
    config: Option<ClientConfig>,
}

impl EmpClientBuilder {
    /// Which brokering system to talk to. Defaults to [`HubjectEnv::Qa`].
    #[must_use]
    pub fn environment(mut self, environment: HubjectEnv) -> Self {
        self.environment = Some(environment);
        self
    }

    /// The provider this client acts as. Goes in every URL path.
    #[must_use]
    pub fn provider_id(mut self, provider_id: ProviderId) -> Self {
        self.provider_id = Some(provider_id);
        self
    }

    /// The client certificate and key Hubject issued.
    #[must_use]
    pub fn identity(mut self, identity: ClientIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    /// The full configuration.
    #[must_use]
    pub fn config(mut self, config: ClientConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Builds the client.
    ///
    /// # Errors
    ///
    /// Returns [`OicpError::Transport`] when no provider id was given, or the TLS identity cannot
    /// be used.
    pub fn build(self) -> Result<EmpClient> {
        let provider_id = self.provider_id.ok_or_else(|| {
            OicpError::transport("an EmpClient needs a ProviderID: it goes in every URL path")
        })?;
        let mut config = self.config.unwrap_or_default();
        if let Some(environment) = self.environment {
            config.environment = environment;
        }
        let identity_warning = warn_on_identity_mismatch(self.identity.as_ref(), provider_id.as_str());
        let transport = Transport::new(config, self.identity.as_ref())?;
        Ok(EmpClient { transport, provider_id, identity_warning })
    }
}

impl EmpClient {
    /// A builder.
    #[must_use]
    pub fn builder() -> EmpClientBuilder {
        EmpClientBuilder::default()
    }

    /// The provider this client acts as.
    #[must_use]
    pub const fn provider_id(&self) -> &ProviderId {
        &self.provider_id
    }

    /// The underlying transport.
    #[must_use]
    pub const fn transport(&self) -> &Transport {
        &self.transport
    }

    /// The certificate mismatch found at construction, if there was one.
    #[must_use]
    pub const fn identity_warning(&self) -> Option<&IdentityWarning> {
        self.identity_warning.as_ref()
    }

    fn path_id(&self) -> PathId {
        PathId::Provider(self.provider_id.clone())
    }

    // --- EVSE data ------------------------------------------------------------------------

    /// Fetches one page of EVSE data.
    ///
    /// Prefer [`crawl_evse_data`](Self::crawl_evse_data) unless you are managing paging yourself.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn pull_evse_data_page(
        &self,
        request: &PullEvseDataRequest,
        query: PageQuery,
    ) -> Result<Page<PullEvseDataRecord>> {
        let page = self.pull_evse_data_page_raw(request, query).await?;
        let mut index = 0_usize;
        page.try_map(|raw| {
            let decoded = serde_json::from_str(raw.get()).map_err(|error| OicpError::Decode {
                pointer: Some(format!("/content/{index}")),
                message: error.to_string(),
            });
            index += 1;
            decoded
        })
    }

    /// Fetches one page of EVSE data with the records left as raw JSON.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn pull_evse_data_page_raw(
        &self,
        request: &PullEvseDataRequest,
        query: PageQuery,
    ) -> Result<Page<Box<RawValue>>> {
        if self.transport.config().validate_requests {
            request.validate()?;
        }
        let base =
            Operation::PullEvseData.url(self.transport.config().environment.base_url(), &self.path_id())?;
        let url = query.append_to(&base);
        self.transport.post_raw(Operation::PullEvseData, &url, request).await
    }

    /// Crawls every page of a `PullEvseData`, yielding records.
    ///
    /// # Why a stream
    ///
    /// An unfiltered European pull is hundreds of thousands of records and hundreds of megabytes.
    /// This holds one page at a time, and yields per record — so a caller can write straight into
    /// a database without ever materialising the set.
    ///
    /// A record that fails to decode is yielded as [`CrawlError::Record`] and the crawl continues:
    /// one operator's malformed record costs one record, not the page. A page that fails to fetch
    /// is [`CrawlError::Page`] and ends the crawl, because continuing past it would silently skip
    /// data.
    ///
    /// ```no_run
    /// # use futures_util::StreamExt;
    /// # use oicp_kit::client::EmpClient;
    /// # use oicp_kit::emp::PullEvseDataRequest;
    /// # use oicp_kit::transport::PageQuery;
    /// # use oicp_kit::types::GeoCoordinatesFormat;
    /// # async fn example(client: &EmpClient) -> Result<(), Box<dyn std::error::Error>> {
    /// let request = PullEvseDataRequest::full(client.provider_id().clone(), GeoCoordinatesFormat::Google);
    /// let mut stream = Box::pin(client.crawl_evse_data(request, PageQuery::new()));
    ///
    /// let mut good = 0_u64;
    /// while let Some(item) = stream.next().await {
    ///     match item {
    ///         Ok(record) => { good += 1; /* store it */ }
    ///         Err(error) => eprintln!("skipping: {error}"),
    ///     }
    /// }
    /// # let _ = good;
    /// # Ok(())
    /// # }
    /// ```
    pub fn crawl_evse_data(
        &self,
        request: PullEvseDataRequest,
        start: PageQuery,
    ) -> impl Stream<Item = core::result::Result<PullEvseDataRecord, CrawlError>> + '_ {
        futures_util::stream::unfold(Some(start), move |state| {
            let request = request.clone();
            async move {
                let query = state?;
                match self.pull_evse_data_page_raw(&request, query).await {
                    Err(source) => Some((vec![Err(CrawlError::Page { page: query.page, source })], None)),
                    Ok(page) => Some(decode_page(page, query)),
                }
            }
        })
        .flat_map(futures_util::stream::iter)
    }

    // --- EVSE status ----------------------------------------------------------------------

    /// Fetches the status of every charging point this EMP can see.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn pull_evse_status(&self, request: &PullEvseStatusRequest) -> Result<EvseStatusResponse> {
        self.transport.post(Operation::PullEvseStatus, &self.path_id(), request).await
    }

    /// Fetches the status of specific charging points — at most 100 per call.
    ///
    /// # Errors
    ///
    /// See [`OicpError`]. A request with more than 100 ids is refused locally.
    pub async fn pull_evse_status_by_id(
        &self,
        request: &PullEvseStatusByIdRequest,
    ) -> Result<EvseStatusByIdResponse> {
        self.transport.post(Operation::PullEvseStatus, &self.path_id(), request).await
    }

    /// Fetches the status of every charging point of specific operators.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn pull_evse_status_by_operator_id(
        &self,
        request: &PullEvseStatusByOperatorIdRequest,
    ) -> Result<EvseStatusResponse> {
        self.transport.post(Operation::PullEvseStatus, &self.path_id(), request).await
    }

    // --- remote control -------------------------------------------------------------------

    /// Starts a charging session remotely.
    ///
    /// **Not retried by default.** A duplicate could start a second session — see
    /// [`RetryPolicy`](super::RetryPolicy).
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn authorize_remote_start(
        &self,
        request: &AuthorizeRemoteStartRequest,
    ) -> Result<Acknowledgement> {
        self.transport.post(Operation::AuthorizeRemoteStart, &self.path_id(), request).await
    }

    /// Stops a charging session it started remotely.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn authorize_remote_stop(
        &self,
        request: &AuthorizeRemoteStopRequest,
    ) -> Result<Acknowledgement> {
        self.transport.post(Operation::AuthorizeRemoteStop, &self.path_id(), request).await
    }

    // --- records --------------------------------------------------------------------------

    /// Fetches one page of charge detail records.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn get_charge_detail_records(
        &self,
        request: &GetChargeDetailRecordsRequest,
        query: PageQuery,
    ) -> Result<ChargeDetailRecordsResponse> {
        self.transport
            .post_raw(Operation::GetChargeDetailRecords, &self.cdr_url(request, query)?, request)
            .await
    }

    /// Fetches one page of charge detail records with the records left as raw JSON.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn get_charge_detail_records_raw(
        &self,
        request: &GetChargeDetailRecordsRequest,
        query: PageQuery,
    ) -> Result<Page<Box<RawValue>>> {
        self.transport
            .post_raw(Operation::GetChargeDetailRecords, &self.cdr_url(request, query)?, request)
            .await
    }

    fn cdr_url(&self, request: &GetChargeDetailRecordsRequest, query: PageQuery) -> Result<String> {
        if self.transport.config().validate_requests {
            request.validate()?;
        }
        let base = Operation::GetChargeDetailRecords
            .url(self.transport.config().environment.base_url(), &self.path_id())?;
        Ok(query.append_to(&base))
    }

    /// Crawls every page of a `GetChargeDetailRecords`, yielding records.
    ///
    /// The reconciliation counterpart of [`crawl_evse_data`](Self::crawl_evse_data): a month of
    /// CDRs for a large EMP is far more than one page, and the failure mode of getting it wrong is
    /// an invoice that does not balance.
    ///
    /// A record that fails to decode is yielded as [`CrawlError::Record`] and the crawl continues;
    /// a page that fails to fetch ends it, because continuing would silently skip settled sessions.
    ///
    /// ```no_run
    /// # use futures_util::StreamExt;
    /// # use oicp_kit::client::EmpClient;
    /// # use oicp_kit::emp::GetChargeDetailRecordsRequest;
    /// # use oicp_kit::transport::PageQuery;
    /// # async fn example(client: &EmpClient, request: GetChargeDetailRecordsRequest)
    /// # -> Result<(), Box<dyn std::error::Error>> {
    /// let mut stream = Box::pin(client.crawl_charge_detail_records(request, PageQuery::new()));
    /// while let Some(item) = stream.next().await {
    ///     let cdr = item?;
    ///     // …reconcile it…
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn crawl_charge_detail_records(
        &self,
        request: GetChargeDetailRecordsRequest,
        start: PageQuery,
    ) -> impl Stream<Item = core::result::Result<ChargeDetailRecord, CrawlError>> + '_ {
        futures_util::stream::unfold(Some(start), move |state| {
            let request = request.clone();
            async move {
                let query = state?;
                match self.get_charge_detail_records_raw(&request, query).await {
                    Err(source) => Some((vec![Err(CrawlError::Page { page: query.page, source })], None)),
                    Ok(page) => Some(decode_page(page, query)),
                }
            }
        })
        .flat_map(futures_util::stream::iter)
    }

    /// Uploads an offline EMP's card base.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn push_authentication_data(
        &self,
        request: &PushAuthenticationDataRequest,
    ) -> Result<Acknowledgement> {
        self.transport.post(Operation::PushAuthenticationData, &self.path_id(), request).await
    }

    // --- pricing --------------------------------------------------------------------------

    /// Fetches the tariffs operators have published to this EMP.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn pull_pricing_product_data(
        &self,
        request: &PullPricingProductDataRequest,
    ) -> Result<PricingProductDataResponse> {
        self.transport.post(Operation::PullPricingProductData, &self.path_id(), request).await
    }

    /// Fetches which tariffs apply at which charging points.
    ///
    /// # Errors
    ///
    /// See [`OicpError`].
    pub async fn pull_evse_pricing(&self, request: &PullEvsePricingRequest) -> Result<EvsePricingResponse> {
        self.transport.post(Operation::PullEvsePricing, &self.path_id(), request).await
    }
}

// Fixtures come from `testkit::samples`, so these tests compile when that feature is on.
#[cfg(all(test, feature = "testkit"))]
mod tests {
    use super::*;

    #[test]
    fn a_client_without_a_provider_id_says_why_it_cannot_be_built() {
        let err = EmpClient::builder().build().unwrap_err();
        assert!(err.to_string().contains("ProviderID"), "{err}");
    }

    #[test]
    fn a_client_builds_and_keeps_its_identifier_verbatim() {
        let client = EmpClient::builder().provider_id("DE-DCB".parse().unwrap()).build().unwrap();
        assert_eq!(client.provider_id().as_str(), "DE-DCB");
    }

    use crate::testkit::samples;

    /// A page of raw records with the given paging fields.
    fn raw_page(number: u32, total_pages: u32, last: bool, records: &[&str]) -> Page<Box<RawValue>> {
        let content: Vec<Box<RawValue>> =
            records.iter().map(|r| RawValue::from_string((*r).to_owned()).unwrap()).collect();
        Page {
            number_of_elements: u32::try_from(content.len()).unwrap(),
            content,
            number,
            size: 2,
            total_elements: u64::from(total_pages) * 2,
            total_pages,
            first: number == 0,
            last,
            empty: None,
            pageable: None,
            status_code: None,
            extensions: crate::types::Extensions::new(),
        }
    }

    #[test]
    fn one_malformed_record_costs_one_record_and_not_the_page() {
        // The property the crawl is documented to have. It was documented before it was true:
        // decoding `Page<PullEvseDataRecord>` in one step fails the whole page on any bad record,
        // and a failed page ends the crawl.
        let good = serde_json::to_string(&samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        let page = raw_page(0, 1, true, &[&good, r#"{"EvseID":"DE*ABC*E2"}"#, &good]);

        let (items, next) = decode_page::<PullEvseDataRecord>(page, PageQuery::at(0, 2));
        assert!(next.is_none());
        assert_eq!(items.len(), 3);
        assert!(items[0].is_ok());
        assert!(items[2].is_ok(), "the record after the bad one still arrives");
        let Err(CrawlError::Record { page, index, .. }) = &items[1] else {
            panic!("expected a per-record failure, got {:?}", items[1]);
        };
        assert_eq!((*page, *index), (0, 1));
    }

    #[test]
    fn a_last_flag_that_would_truncate_a_crawl_does_not() {
        // `last: true` on page 0 of 3 would end the crawl after a third of the data, silently.
        let good = serde_json::to_string(&samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        let (items, next) =
            decode_page::<PullEvseDataRecord>(raw_page(0, 3, true, &[&good]), PageQuery::at(0, 2));

        assert_eq!(next, Some(PageQuery::at(1, 2)), "the crawl goes on by totalPages");
        assert!(matches!(items[0], Err(CrawlError::PageInconsistent { page: 0, .. })));
        assert!(items[1].is_ok(), "and the page's records still arrive");
    }

    #[test]
    fn a_last_flag_that_would_never_end_is_bounded_by_total_pages() {
        // The mirror of the truncation case: `last: false` on the final page. Following it walks
        // pages that do not exist, forever.
        let good = serde_json::to_string(&samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        let (items, next) =
            decode_page::<PullEvseDataRecord>(raw_page(2, 3, false, &[&good]), PageQuery::at(2, 2));

        assert_eq!(next, None, "totalPages ends the crawl");
        assert!(matches!(items[0], Err(CrawlError::PageInconsistent { page: 2, .. })));
        assert!(items[1].is_ok());
    }

    #[test]
    fn an_empty_page_that_claims_more_ends_the_crawl_rather_than_looping() {
        let (items, next) =
            decode_page::<PullEvseDataRecord>(raw_page(0, 9, false, &[]), PageQuery::at(0, 2));
        assert_eq!(next, None);
        assert!(matches!(items[0], Err(CrawlError::PageInconsistent { .. })));
    }

    #[test]
    fn an_ordinary_page_asks_for_the_next_one_at_the_same_size() {
        let good = serde_json::to_string(&samples::pull_evse_data_record("DE*ABC*E1")).unwrap();
        let (items, next) =
            decode_page::<PullEvseDataRecord>(raw_page(1, 3, false, &[&good]), PageQuery::at(1, 500));
        assert_eq!(next, Some(PageQuery::at(2, 500)));
        assert_eq!(items.len(), 1);
        assert!(items[0].is_ok());
    }
}
