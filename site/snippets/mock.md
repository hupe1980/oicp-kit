```rust
let mut hubject = MockHubject::new();
hubject.register_emp(MockEmp::permissive("DE-DCB".parse()?));
hubject.push_evse_data(&samples::evse_data_record("DE*ABC*E1").into())?;

// A driver swipes a card. The broker routes on the contract, exactly as the real one does.
let response = hubject.authorize_start(&samples::authorize_start_request("DE*ABC*E1"));
let session = response.session_id.clone().unwrap();

// A CDR for a session the broker opened settles…
hubject.submit_cdr(&samples::charge_detail_record("DE*ABC*E1", session))?;

// …and one for a session it did not is refused, with the specification's own code.
let invented = samples::charge_detail_record("DE*ABC*E1", samples::session_id());
assert_eq!(*hubject.submit_cdr(&invented).unwrap_err().code(), Code::SessionIsInvalid);
```
