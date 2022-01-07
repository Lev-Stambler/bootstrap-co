use near_sdk::env;

pub(crate) fn assert_1_yocto() {
    // TODO: in sep function
    assert_eq!(env::attached_deposit(), 1, "Expected an attached deposit of 1");
}
