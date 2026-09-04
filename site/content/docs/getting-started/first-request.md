+++
title = "Your first request"
weight = 30
description = "From a certificate to a push, and then to the half Hubject calls back."
+++

## 1. The certificate

OICP authenticates with mutual TLS. Hubject issues you a client certificate; put the certificate
and its key in one PEM file.

```rust
use oicp_kit::client::{ClientIdentity, CpoClient};
use oicp_kit::transport::HubjectEnv;

let identity = ClientIdentity::from_pem_file("hubject-client.pem")?;

let client = CpoClient::builder()
    .environment(HubjectEnv::Qa)          // the default; `Prod` is opt-in
    .operator_id("DE*ABC".parse()?)
    .identity(identity)
    .build()?;
```

The builder compares your `OperatorID` against the certificate's names and logs a warning if it
does not appear. That comparison is the one Hubject makes on every request, and its failure mode —
`017 Unauthorized Access` on everything, with no clue which side is wrong — is worth catching at
startup:

```rust
if let Some(warning) = client.identity_warning() {
    eprintln!("{warning}");   // and decide whether to start
}
```

Note that a warning is not always an error: a **hub operator** legitimately acts for bundled
sub-partners whose identifiers are not in its certificate.

## 2. Publish a charging point

```rust
use oicp_kit::cpo::EvseDataRecord;
use oicp_kit::types::*;

let record = EvseDataRecord::builder()
    .evse_id("DE*ABC*E1".parse()?)
    .charging_station_names(vec![InfoText::new("en", "Market Square")?])
    .address(
        Address::builder()
            .country("DEU")               // alpha-3; OICP 2.2 and 2.3 allow only alpha-3
            .city("Berlin")
            .street("EUREF CAMPUS")
            .postal_code("10829")
            .house_num("22")
            .time_zone("UTC+01:00")
            .build()?,
    )
    .geo_coordinates(GeoCoordinates::Google { coordinates: "52.480495 13.356465".into() })
    .plugs(vec![Plug::Type2Outlet])
    .charging_facilities(vec![
        ChargingFacility::builder().power_type(PowerType::Ac3Phase).power(Number::from(22)).build()?
    ])
    .renewable_energy(true)
    .calibration_law_data_availability(CalibrationLawDataAvailability::Local)
    .authentication_modes(vec![AuthenticationMode::NfcRfidClassic])
    .payment_options(vec![PaymentOption::Contract])
    .value_added_services(vec![ValueAddedService::None])
    .accessibility(Accessibility::FreePubliclyAccessible)
    .hotline_phone_number("+49301234567")
    .is_open_24_hours(true)
    .is_hubject_compatible(true)
    .dynamic_info_available(DynamicInfoAvailable::True)
    .build()?;                            // ← validates; returns Err(Violations)

client.push_evse_data_insert(vec![record], "ABC Technologies").await?;
```

`build()` checks the finished object against the specification, so a record that would be rejected
never leaves the process. `build_unchecked()` skips the check and says so in its name.

Note what is **not** here: `push_evse_data_insert`, not `fullLoad`. See
[Delta sync](@/docs/layers/sync.md) for why that matters.

## 3. Serve the half Hubject calls

A CPO that stops here cannot be started from a phone app.

```rust
use oicp_kit::cpo::*;
use oicp_kit::server::{CpoService, cpo_router};
use oicp_kit::types::{Acknowledgement, Code};

struct MyCpo { /* your charge point manager */ }

impl CpoService for MyCpo {
    async fn authorize_remote_start(&self, request: AuthorizeRemoteStartRequest) -> Acknowledgement {
        match self.start_session(&request.evse_id).await {
            Ok(()) => Acknowledgement::success().with_session(request.session_id),
            Err(_)  => Acknowledgement::failure(Code::CommunicationToEvseFailed),
        }
    }
    async fn authorize_remote_stop(&self, request: AuthorizeRemoteStopRequest) -> Acknowledgement {
        Acknowledgement::success().with_session(request.session_id)
    }
    async fn reservation_start(&self, _: AuthorizeRemoteReservationStartRequest) -> Acknowledgement {
        Acknowledgement::failure(Code::ServiceNotAvailable)   // honest: we do not offer this
    }
    async fn reservation_stop(&self, _: AuthorizeRemoteReservationStopRequest) -> Acknowledgement {
        Acknowledgement::failure(Code::ServiceNotAvailable)
    }
}

let app = cpo_router(std::sync::Arc::new(MyCpo { /* … */ }));
axum::serve(tokio::net::TcpListener::bind("0.0.0.0:8443").await?, app).await?;
```

Register the resulting URLs in the Hubject portal. The paths come from `transport::Operation`, so
`oicp endpoints --id 'DE*ABC'` prints exactly what to enter.

## 4. Test it before you onboard

```rust
use oicp_kit::testkit::{MockHubject, MockEmp, samples};

let mut hubject = MockHubject::new();
hubject.register_emp(MockEmp::permissive("DE-DCB".parse()?));
hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())?;

let request = hubject.remote_start(
    &"DE-DCB".parse()?, &"DE*ABC*E1".parse()?, &"DE-DCB-C12345678-X".parse()?,
)?;

// This is exactly what your CpoService will receive.
assert!(my_cpo.authorize_remote_start(request).await.is_success());
```

See [Testkit](@/docs/layers/testkit.md).
