use halvo_coord::types::Limits;
use halvo_coord_store::HISTORY_PAGE;
use halvo_schema::coordinator::{HISTORY_RECORDS, REQUEST_BYTES, REQUEST_RECORDS};

#[test]
fn the_bounds_equal_the_product_limits_and_the_history_page() {
    assert_eq!(
        REQUEST_RECORDS,
        Limits::PRODUCT.request_records as usize,
        "REQUEST_RECORDS and request_records of Limits::PRODUCT"
    );
    assert_eq!(
        REQUEST_BYTES,
        Limits::PRODUCT.request_bytes as usize,
        "REQUEST_BYTES and request_bytes of Limits::PRODUCT"
    );
    assert_eq!(
        HISTORY_PAGE, HISTORY_RECORDS,
        "the history page and HISTORY_RECORDS"
    );
}
