#[fuzz_fixture]
impl RoshiFixture {
    include!("fixture/setup.rs");
    include!("fixture/deposits.rs");
    include!("fixture/withdrawals.rs");
    include!("fixture/execution.rs");
    include!("fixture/nav_fees.rs");
    include!("fixture/authority_config.rs");
    include!("fixture/controls.rs");
}
