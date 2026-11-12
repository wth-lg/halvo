use halvo_schema::executable::Limits;
use halvo_schema::SeedCategory;

/// The limits of the agents of each seed category, `steps`, `wall_s` and `model_tokens` (A1.3), and beside the rows
/// the most rebinds of one effect, the same for every category (A5.2, A7.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitTable {
    pub utility_bill_payment: Limits,
    pub tenant_rent: Limits,
    pub lease_renewal_or_change: Limits,
    pub tenant_application_review: Limits,
    pub email: Limits,
    pub spreadsheet_fill: Limits,
    pub shop_order: Limits,
    pub calendar_cleanup: Limits,
    pub ride_order: Limits,
    pub daily_chart_check: Limits,
    /// The most `rebind` rows of one effect: a further `rebind` is refused with `changed`.
    pub rebinds: u32,
}

impl LimitTable {
    /// The row of `category`.
    pub fn of(&self, category: SeedCategory) -> Limits {
        let row = match category {
            SeedCategory::UtilityBillPayment => &self.utility_bill_payment,
            SeedCategory::TenantRent => &self.tenant_rent,
            SeedCategory::LeaseRenewalOrChange => &self.lease_renewal_or_change,
            SeedCategory::TenantApplicationReview => &self.tenant_application_review,
            SeedCategory::Email => &self.email,
            SeedCategory::SpreadsheetFill => &self.spreadsheet_fill,
            SeedCategory::ShopOrder => &self.shop_order,
            SeedCategory::CalendarCleanup => &self.calendar_cleanup,
            SeedCategory::RideOrder => &self.ride_order,
            SeedCategory::DailyChartCheck => &self.daily_chart_check,
        };
        row.clone()
    }
}

const TASK: Limits = Limits {
    steps: 20,
    wall_s: 120,
    model_tokens: 0,
};

const DEFERRED: Limits = Limits {
    steps: 1,
    wall_s: 1,
    model_tokens: 0,
};

/// The table. Until the trial file of the driver exists, every category with a task has 20, 120 and 0, and a
/// deferred category 1, 1 and 0 (A1.3).
pub const LIMITS: LimitTable = LimitTable {
    utility_bill_payment: TASK,
    tenant_rent: DEFERRED,
    lease_renewal_or_change: DEFERRED,
    tenant_application_review: DEFERRED,
    email: TASK,
    spreadsheet_fill: TASK,
    shop_order: TASK,
    calendar_cleanup: TASK,
    ride_order: DEFERRED,
    daily_chart_check: DEFERRED,
    rebinds: 2,
};
