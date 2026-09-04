//! `oicp` — a command line tool for the things you do while integrating.

use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use oicp_kit::cpo::{ChargeDetailRecord, ChargingNotification, EvseDataRecord, PushEvseDataRequest};
use oicp_kit::eichrecht::{CdrCheck, Severity};
use oicp_kit::emp::{PullEvseDataRecord, PullEvseDataRequest};
use oicp_kit::testkit::scenarios;
use oicp_kit::transport::{HubjectEnv, Involvement, Operation, Role};
use oicp_kit::types::{
    Acknowledgement, ERRATA, EvcoId, EvseId, OperatorId, ProviderId, SessionId, Uid, Validate,
};

/// A toolkit for OICP 2.3, the roaming protocol of the Hubject brokering system.
#[derive(Parser)]
#[command(name = "oicp", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check a JSON payload against the specification.
    ///
    /// Reads from a file or from standard input, and reports every violation with a JSON Pointer.
    Validate {
        /// Which kind of message it is.
        #[arg(value_enum)]
        kind: MessageKind,
        /// The file to read; `-` or absent reads standard input.
        file: Option<String>,
    },
    /// Parse an OICP identifier and say what it means.
    ///
    /// The two grammars — ISO 15118 and DIN SPEC 91286 — are easy to confuse, and Hubject matches
    /// identifiers against your TLS certificate as text.
    Id {
        /// The identifier.
        value: String,
    },
    /// Check a charge detail record before submitting it.
    ///
    /// Catches what Hubject and the EMP would reject weeks later, including the rules that need
    /// the charging point's own data record.
    Cdr {
        /// The CDR to check.
        file: Option<String>,
        /// The EVSE data record to check it against, for the calibration-law rules.
        #[arg(long)]
        evse: Option<String>,
        /// The tariff products this operator has published.
        #[arg(long, value_delimiter = ',')]
        products: Vec<String>,
    },
    /// Print the endpoint table, or the URL for one operation.
    Endpoints {
        /// Which Hubject environment.
        #[arg(long, value_enum, default_value_t = Environment::Qa)]
        environment: Environment,
        /// Which side you are. A CPO and an EMP use different endpoints, and the ones each of them
        /// *serves* differ too — the same path is called by one role and implemented by the other.
        #[arg(long, value_enum, default_value_t = PartyRole::Cpo)]
        role: PartyRole,
        /// Your own identifier, substituted into the paths you call.
        #[arg(long)]
        id: Option<String>,
    },
    /// Run the onboarding scenarios against an in-process broker.
    Scenarios,
    /// Print the places Hubject's own OICP 2.3 documents disagree with each other.
    Errata,
    /// Print the places OICP 2.3 contradicts real charging hardware.
    Defects,
    /// Say whether a charging point is open at a given instant.
    ///
    /// OICP's opening times are local times, and the offset to read them with is in the address.
    Open {
        /// The EVSE data record.
        file: Option<String>,
        /// The instant to ask about, as RFC 3339. Defaults to now.
        #[arg(long)]
        at: Option<String>,
    },
    /// Write the JSON Schema of every wire type.
    Schema {
        /// The directory to write into. Defaults to standard output as one document.
        #[arg(long)]
        out: Option<String>,
    },
    /// Serve a Hubject brokering system on a local port, for integration testing.
    ///
    /// No certificates, no contract, no QA environment. Point your client at the printed URL.
    ServeMock {
        /// The address to bind.
        #[arg(long, default_value = "127.0.0.1:8080")]
        bind: String,
        /// Seed the broker with this many charging points for `DE*ABC`.
        #[arg(long, default_value_t = 3)]
        fleet: u32,
        /// The EMP to register, whose contracts authorize.
        #[arg(long, default_value = "DE-DCB")]
        provider: String,
    },
    /// Crawl EVSE data into a local snapshot, using the delta engine.
    Pull {
        /// The EMP doing the pulling.
        #[arg(long)]
        provider: String,
        /// The base URL. Use `oicp serve-mock` to try it without onboarding.
        #[arg(long)]
        url: String,
        /// The snapshot file to keep the copy in.
        #[arg(long, default_value = "evse-snapshot.json")]
        snapshot: String,
        /// The client certificate and key, in one PEM file. Required against a real Hubject.
        #[arg(long)]
        identity: Option<String>,
        /// Records per page.
        #[arg(long, default_value_t = 2000)]
        page_size: u32,
        /// Ignore the watermark and pull everything.
        #[arg(long)]
        rebaseline: bool,
    },
}

#[derive(Copy, Clone, ValueEnum)]
enum MessageKind {
    /// An `EvseDataRecord`, as a CPO pushes it.
    EvseData,
    /// A `PullEvseDataRecord`, as an EMP receives it.
    PullEvseData,
    /// An `eRoamingPushEvseData` request.
    PushEvseData,
    /// An `eRoamingPullEvseData` request.
    PullEvseDataRequest,
    /// An `eRoamingChargeDetailRecord`.
    Cdr,
    /// An `eRoamingChargingNotification`.
    Notification,
    /// An `eRoamingAcknowledgment`.
    Acknowledgement,
}

/// Which side of the roaming relationship you are.
#[derive(Copy, Clone, ValueEnum)]
enum PartyRole {
    /// A Charge Point Operator.
    Cpo,
    /// An e-Mobility Provider.
    Emp,
}

impl From<PartyRole> for Role {
    fn from(role: PartyRole) -> Self {
        match role {
            PartyRole::Cpo => Self::Cpo,
            PartyRole::Emp => Self::Emp,
        }
    }
}

#[derive(Copy, Clone, ValueEnum)]
enum Environment {
    /// The production brokering system.
    Prod,
    /// The QA brokering system.
    Qa,
}

impl From<Environment> for HubjectEnv {
    fn from(environment: Environment) -> Self {
        match environment {
            Environment::Prod => Self::Prod,
            Environment::Qa => Self::Qa,
        }
    }
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command {
        Command::Validate { kind, file } => validate(kind, file.as_deref()),
        Command::Id { value } => identify(&value),
        Command::Cdr { file, evse, products } => check_cdr(file.as_deref(), evse.as_deref(), &products),
        Command::Endpoints { environment, role, id } => {
            endpoints(environment.into(), role.into(), id.as_deref());
            Ok(())
        }
        Command::Scenarios => run_scenarios(),
        Command::Errata => {
            print_errata();
            Ok(())
        }
        Command::Defects => {
            print_defects();
            Ok(())
        }
        Command::Open { file, at } => opening(file.as_deref(), at.as_deref()),
        Command::Schema { out } => schema(out.as_deref()),
        Command::ServeMock { bind, fleet, provider } => serve_mock(&bind, fleet, &provider),
        Command::Pull { provider, url, snapshot, identity, page_size, rebaseline } => {
            pull(&provider, &url, &snapshot, identity.as_deref(), page_size, rebaseline)
        }
    }
}

fn read_input(file: Option<&str>) -> Result<String, String> {
    match file {
        None | Some("-") => {
            let mut buffer = String::new();
            std::io::stdin().read_to_string(&mut buffer).map_err(|e| format!("stdin: {e}"))?;
            Ok(buffer)
        }
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}")),
    }
}

fn validate(kind: MessageKind, file: Option<&str>) -> Result<(), String> {
    let text = read_input(file)?;

    macro_rules! check {
        ($ty:ty) => {{
            let value: $ty = serde_json::from_str(&text)
                .map_err(|e| format!("this is not the message it should be: {e}"))?;
            report(value.validate())
        }};
    }

    match kind {
        MessageKind::EvseData => check!(EvseDataRecord),
        MessageKind::PullEvseData => check!(PullEvseDataRecord),
        MessageKind::PushEvseData => check!(PushEvseDataRequest),
        MessageKind::PullEvseDataRequest => check!(PullEvseDataRequest),
        MessageKind::Cdr => check!(ChargeDetailRecord),
        MessageKind::Notification => check!(ChargingNotification),
        MessageKind::Acknowledgement => check!(Acknowledgement),
    }
}

fn report(result: Result<(), oicp_kit::types::Violations>) -> Result<(), String> {
    match result {
        Ok(()) => {
            println!("conformant");
            Ok(())
        }
        Err(violations) => {
            let mut message = format!("{} violation(s):\n", violations.len());
            for violation in &violations {
                let at = if violation.pointer.is_empty() { "/" } else { &violation.pointer };
                let _ = writeln!(message, "  {at}\n    [{}] {}", violation.code, violation.message);
            }
            Err(message)
        }
    }
}

fn identify(value: &str) -> Result<(), String> {
    let mut found = false;

    if let Ok(id) = value.parse::<EvseId>() {
        println!("EvseID     {value}");
        println!("  standard {}", id.standard());
        println!("  country  {}", id.country());
        println!("  operator {}", id.operator_id());
        println!("  key      {}", id.canonical());
        found = true;
    }
    if let Ok(id) = value.parse::<EvcoId>() {
        println!("EvcoID     {value}");
        println!("  standard {}", id.standard());
        println!("  provider {}", id.provider_id());
        println!("  key      {}", id.canonical());
        found = true;
    }
    if let Ok(id) = value.parse::<OperatorId>() {
        println!("OperatorID {value}");
        println!("  standard {}", id.standard());
        println!("  country  {}", id.country());
        found = true;
    }
    if let Ok(id) = value.parse::<ProviderId>() {
        println!("ProviderID {value}");
        println!("  standard {} (the two grammars coincide for a ProviderID)", id.standard());
        println!("  country  {}", id.country());
        found = true;
    }
    if value.parse::<SessionId>().is_ok() {
        println!("SessionID  {value}");
        found = true;
    }
    if value.parse::<Uid>().is_ok() {
        println!("RFID UID   {value}");
        found = true;
    }

    if found { Ok(()) } else { Err(format!("{value:?} is not a well-formed OICP identifier of any kind")) }
}

fn check_cdr(file: Option<&str>, evse: Option<&str>, products: &[String]) -> Result<(), String> {
    let cdr: ChargeDetailRecord = serde_json::from_str(&read_input(file)?)
        .map_err(|e| format!("this is not a charge detail record: {e}"))?;

    let evse_record = match evse {
        None => None,
        Some(path) => Some(
            serde_json::from_str::<EvseDataRecord>(
                &std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?,
            )
            .map_err(|e| format!("{path} is not an EVSE data record: {e}"))?,
        ),
    };

    let mut check = CdrCheck::new();
    if let Some(record) = &evse_record {
        check = check.against_evse(record);
    }
    if !products.is_empty() {
        check = check.with_known_products(products.iter().cloned());
    }

    let findings = check.run(&cdr);
    if findings.is_empty() {
        println!("submittable");
        if evse_record.is_none() {
            println!("note: pass --evse to check the calibration-law and plausibility rules too");
        }
        return Ok(());
    }

    let errors = findings.iter().filter(|f| f.severity == Severity::Error).count();
    for finding in &findings {
        println!("{finding}");
    }
    if errors == 0 {
        println!("\nsubmittable, with {} warning(s)", findings.len());
        Ok(())
    } else {
        Err(format!("\n{errors} error(s): this CDR will be rejected or disputed"))
    }
}

fn endpoints(environment: HubjectEnv, role: Role, id: Option<&str>) {
    let article = if role == Role::Emp { "an" } else { "a" };
    println!("{environment}  —  as {article} {role}\n");

    let mut serves = vec![];
    for operation in Operation::for_role(role) {
        let involvement = operation.involvement(role).expect("for_role filtered on it");
        // A path you *call* carries your own identifier. A path you *serve* carries the peer's, so
        // the template is the honest thing to print — it is what you register in the portal.
        let path = match (involvement, id) {
            // `ChargingNotifications` has no identifier in its path, so there is nothing to
            // substitute — the base URL and the template are the whole answer.
            (Involvement::YouCall, Some(value)) => operation.caller_path_id(role, value).map_or_else(
                || format!("{}{}", environment.base_url(), operation.path_template()),
                |path_id| operation.url(environment.base_url(), &path_id).unwrap_or_default(),
            ),
            _ => operation.path_template().to_owned(),
        };
        if involvement == Involvement::YouServe {
            serves.push(operation);
        }
        println!("{:<34} {involvement}  {path}", format!("{operation:?}"));
    }

    let trait_name = match role {
        Role::Cpo => "server::CpoService",
        Role::Emp => "server::EmpService",
    };
    println!(
        "\nThe {} marked `Hubject -> you` are endpoints you serve: register their paths in the",
        serves.len()
    );
    println!("Hubject portal and implement {trait_name}. The identifier in those paths is the");
    println!("peer's, not yours, which is why they are shown as templates.");
    if id.is_none() {
        println!("\nPass --id to see the paths you call with your own identifier substituted in.");
    }
}

fn run_scenarios() -> Result<(), String> {
    let report = scenarios::run_all();
    println!("{report}");
    if report.passed() { Ok(()) } else { Err(format!("\n{} scenario(s) failed", report.failures())) }
}

fn print_errata() {
    println!("Where Hubject's own OICP 2.3 documents disagree with each other.\n");
    for erratum in ERRATA {
        println!("{}  {}", erratum.id, erratum.field);
        println!("  leading document: {}", erratum.leading_document);
        println!("  OpenAPI schema:   {}", erratum.openapi_document);
        println!("  impact:           {}", erratum.impact);
        println!("  oicp-kit:         {}\n", erratum.resolution);
    }
}

fn print_defects() {
    println!("Where OICP 2.3 contradicts real charging hardware.\n");
    for defect in oicp_kit::types::SPEC_DEFECTS {
        println!("{}  {}", defect.id, defect.field);
        println!("  specification: {}", defect.specification_says);
        println!("  reality:       {}", defect.reality);
        println!("  consequence:   {}", defect.consequence);
        println!("  reported at:   {}", defect.upstream_issue);
        println!("  oicp-kit:      {}\n", defect.resolution);
    }
}

fn opening(file: Option<&str>, at: Option<&str>) -> Result<(), String> {
    use oicp_kit::types::{DateTime, Opening};

    let record: EvseDataRecord = serde_json::from_str(&read_input(file)?)
        .map_err(|e| format!("this is not an EVSE data record: {e}"))?;
    let at: DateTime = match at {
        None => DateTime::now(),
        Some(text) => text.parse().map_err(|e| format!("{text:?} is not an RFC 3339 instant: {e}"))?,
    };

    println!("{} at {at}", record.evse_id);
    match record.is_open_at(&at) {
        Opening::Open => {
            println!("  open");
            Ok(())
        }
        Opening::Closed => {
            println!("  closed");
            Ok(())
        }
        Opening::Unknown(reason) => Err(format!(
            "  unknown — {}",
            match reason {
                oicp_kit::types::UnknownReason::NoOpeningTimes =>
                    "the record is not open around the clock and carries no OpeningTimes",
                oicp_kit::types::UnknownReason::NoTimeZone =>
                    "the address carries no TimeZone, so the local time cannot be derived",
                oicp_kit::types::UnknownReason::MalformedTimeZone =>
                    "the TimeZone is not of the form UTC+HH:MM",
                oicp_kit::types::UnknownReason::MalformedPeriod => "an opening period does not parse",
                _ => "the record does not carry enough to decide",
            }
        )),
    }
}

fn schema(out: Option<&str>) -> Result<(), String> {
    let mut generator = schemars::SchemaGenerator::default();
    let mut documents: Vec<(&str, schemars::Schema)> = vec![];

    macro_rules! emit {
        ($($ty:ty),* $(,)?) => {
            $( documents.push((stringify!($ty), generator.root_schema_for::<$ty>())); )*
        };
    }
    emit!(
        EvseDataRecord,
        PullEvseDataRecord,
        PushEvseDataRequest,
        PullEvseDataRequest,
        ChargeDetailRecord,
        ChargingNotification,
        Acknowledgement,
        oicp_kit::cpo::PushEvseStatusRequest,
        oicp_kit::cpo::AuthorizeStartRequest,
        oicp_kit::cpo::AuthorizationStartResponse,
        oicp_kit::cpo::AuthorizeRemoteStartRequest,
        oicp_kit::emp::EvseStatusResponse,
        oicp_kit::emp::GetChargeDetailRecordsRequest,
    );

    match out {
        None => {
            let combined: serde_json::Map<String, serde_json::Value> = documents
                .into_iter()
                .map(|(name, schema)| {
                    (name.rsplit("::").next().unwrap_or(name).to_owned(), schema.to_value())
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&combined).map_err(|e| format!("could not render: {e}"))?
            );
        }
        Some(directory) => {
            std::fs::create_dir_all(directory).map_err(|e| format!("{directory}: {e}"))?;
            for (name, schema) in documents {
                let name = name.rsplit("::").next().unwrap_or(name);
                let path = std::path::Path::new(directory).join(format!("{name}.schema.json"));
                let text =
                    serde_json::to_string_pretty(&schema.to_value()).map_err(|e| format!("{name}: {e}"))?;
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
                println!("{}", path.display());
            }
        }
    }
    Ok(())
}

fn serve_mock(bind: &str, fleet: u32, provider: &str) -> Result<(), String> {
    use oicp_kit::testkit::{MockEmp, MockHubject, MockHubjectServer, samples};

    let provider_id = provider.parse().map_err(|e| format!("{provider:?} is not a ProviderID: {e}"))?;
    let mut hubject = MockHubject::new();
    hubject.register_emp(MockEmp::permissive(provider_id));
    for i in 0..fleet {
        let record = samples::evse_data_record(&format!("DE*ABC*E{i}"));
        hubject.push_evse_data(&record.into()).map_err(|ack| format!("seeding failed: {ack:?}"))?;
    }

    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("could not start a runtime: {e}"))?;
    runtime.block_on(async move {
        let server = MockHubjectServer::bind(hubject, bind)
            .await
            .map_err(|e| format!("could not bind {bind}: {e}"))?;

        println!("a Hubject brokering system is listening.\n");
        println!("  base URL   {}", server.base_url());
        println!("  operator   DE*ABC ({fleet} charging point(s))");
        println!("  provider   {provider}\n");
        println!("Point a client at it:\n");
        println!(
            "  oicp pull --provider {provider} --url {} --snapshot ./snapshot.json\n",
            server.base_url()
        );
        println!("It speaks plain HTTP, so no certificate is needed. Press Ctrl-C to stop.");

        tokio::signal::ctrl_c().await.map_err(|e| format!("could not wait for Ctrl-C: {e}"))?;
        println!("\nstopping");
        server.stop().await;
        Ok(())
    })
}

fn pull(
    provider: &str,
    url: &str,
    snapshot: &str,
    identity: Option<&str>,
    page_size: u32,
    rebaseline: bool,
) -> Result<(), String> {
    use oicp_kit::client::{ClientIdentity, EmpClient};
    use oicp_kit::sync::{self, EvseRepository, FileEvseRepository, Planner, PlannerConfig};
    use oicp_kit::transport::PageQuery;
    use oicp_kit::types::GeoCoordinatesFormat;

    let provider_id: ProviderId =
        provider.parse().map_err(|e| format!("{provider:?} is not a ProviderID: {e}"))?;
    let mut repository = FileEvseRepository::open(snapshot).map_err(|e| e.to_string())?;

    let mut builder =
        EmpClient::builder().environment(HubjectEnv::Custom(url.to_owned())).provider_id(provider_id.clone());
    if let Some(path) = identity {
        builder = builder.identity(ClientIdentity::from_pem_file(path).map_err(|e| e.to_string())?);
    } else if !url.starts_with("http://") {
        return Err("a real Hubject endpoint needs --identity: OICP authenticates with a client \
                    certificate, and there is no token to fall back on"
            .into());
    }
    let client = builder.build().map_err(|e| e.to_string())?;
    if let Some(warning) = client.identity_warning() {
        eprintln!("warning: {warning}");
    }

    let mut config = PlannerConfig::new(provider_id, GeoCoordinatesFormat::Google);
    if rebaseline {
        config = config.rebaseline();
    }
    let planner = Planner::new(config);

    let runtime = tokio::runtime::Runtime::new().map_err(|e| format!("could not start a runtime: {e}"))?;
    runtime.block_on(async {
        let (plan, watermark) = planner.plan(&repository).map_err(|e| e.to_string())?;
        match &plan {
            oicp_kit::sync::Plan::Full { reason, .. } => println!("full pull — {reason}"),
            oicp_kit::sync::Plan::Delta { since, .. } => println!("delta pull — changes since {since}"),
        }
        if plan.replaces_everything() {
            // A full pull *is* the whole world; anything not in it has been withdrawn.
            repository.clear().map_err(|e| e.to_string())?;
        }

        let mut outcome = oicp_kit::sync::ApplyOutcome::default();
        let mut query = Some(PageQuery::with_size(page_size));
        while let Some(current) = query {
            let page = client
                .pull_evse_data_page(plan.request(), current)
                .await
                .map_err(|e| format!("page {}: {e}", current.page))?;
            print!("\r  page {} of {}", page.number + 1, page.total_pages.max(1));
            std::io::stdout().flush().ok();

            query = page.next_page().map(|n| PageQuery::at(n, current.size));
            outcome.merge(sync::apply(&mut repository, page.content).map_err(|e| e.to_string())?);
        }
        println!();

        // Only now: a watermark advanced before the crawl finished loses the unapplied changes
        // permanently, because the next delta starts after them.
        planner.commit(&mut repository, watermark).map_err(|e| e.to_string())?;
        repository.save().map_err(|e| e.to_string())?;

        println!("  {} inserted, {} updated, {} deleted", outcome.inserted, outcome.updated, outcome.deleted);
        if outcome.suggests_drift() {
            println!(
                "  note: {} update(s) and {} deletion(s) were for records this copy did not have, \
                 which means it had drifted",
                outcome.updated_unknown, outcome.deleted_unknown
            );
        }
        println!("  {} record(s) in {snapshot}", repository.len().map_err(|e| e.to_string())?);
        Ok(())
    })
}
