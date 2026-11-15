use halvo_coord::types::Limits;
use halvo_coord_store::HISTORY_PAGE;
use halvo_schema::coordinator::{HISTORY_RECORDS, REQUEST_BYTES, REQUEST_RECORDS};

#[test]
fn the_bounds_hold_at_least_the_product_limits_and_equal_the_history_page() {
    assert!(
        REQUEST_RECORDS >= Limits::PRODUCT.request_records as usize,
        "REQUEST_RECORDS below request_records of Limits::PRODUCT: the handler would refuse what the app sends"
    );
    assert!(
        REQUEST_BYTES >= Limits::PRODUCT.request_bytes as usize,
        "REQUEST_BYTES below request_bytes of Limits::PRODUCT: the handler would refuse what the app sends"
    );
    assert_eq!(
        HISTORY_PAGE, HISTORY_RECORDS,
        "the history page and HISTORY_RECORDS"
    );
}
