use crate::ids::name_enum;

name_enum! {
    /// A category: the 10 seed categories and `other`, which is no seed category. A label row holds a `Category`.
    /// `as_str()`, `Display` and the serde form give the category id, and categories order as in `ALL`.
    pub enum Category {
        UtilityBillPayment = "utility_bill_payment",
        TenantRent = "tenant_rent",
        LeaseRenewalOrChange = "lease_renewal_or_change",
        TenantApplicationReview = "tenant_application_review",
        Email = "email",
        SpreadsheetFill = "spreadsheet_fill",
        ShopOrder = "shop_order",
        CalendarCleanup = "calendar_cleanup",
        RideOrder = "ride_order",
        DailyChartCheck = "daily_chart_check",
        Other = "other",
    }
}

name_enum! {
    /// A seed category: the first 10 categories. It refuses `other`. A summary, a workflow and a proposal hold one.
    pub enum SeedCategory {
        UtilityBillPayment = "utility_bill_payment",
        TenantRent = "tenant_rent",
        LeaseRenewalOrChange = "lease_renewal_or_change",
        TenantApplicationReview = "tenant_application_review",
        Email = "email",
        SpreadsheetFill = "spreadsheet_fill",
        ShopOrder = "shop_order",
        CalendarCleanup = "calendar_cleanup",
        RideOrder = "ride_order",
        DailyChartCheck = "daily_chart_check",
    }
}

impl From<SeedCategory> for Category {
    fn from(seed: SeedCategory) -> Self {
        match seed {
            SeedCategory::UtilityBillPayment => Category::UtilityBillPayment,
            SeedCategory::TenantRent => Category::TenantRent,
            SeedCategory::LeaseRenewalOrChange => Category::LeaseRenewalOrChange,
            SeedCategory::TenantApplicationReview => Category::TenantApplicationReview,
            SeedCategory::Email => Category::Email,
            SeedCategory::SpreadsheetFill => Category::SpreadsheetFill,
            SeedCategory::ShopOrder => Category::ShopOrder,
            SeedCategory::CalendarCleanup => Category::CalendarCleanup,
            SeedCategory::RideOrder => Category::RideOrder,
            SeedCategory::DailyChartCheck => Category::DailyChartCheck,
        }
    }
}

impl Category {
    /// The seed category of this category, or none for `other`.
    pub fn seed(self) -> Option<SeedCategory> {
        match self {
            Category::UtilityBillPayment => Some(SeedCategory::UtilityBillPayment),
            Category::TenantRent => Some(SeedCategory::TenantRent),
            Category::LeaseRenewalOrChange => Some(SeedCategory::LeaseRenewalOrChange),
            Category::TenantApplicationReview => Some(SeedCategory::TenantApplicationReview),
            Category::Email => Some(SeedCategory::Email),
            Category::SpreadsheetFill => Some(SeedCategory::SpreadsheetFill),
            Category::ShopOrder => Some(SeedCategory::ShopOrder),
            Category::CalendarCleanup => Some(SeedCategory::CalendarCleanup),
            Category::RideOrder => Some(SeedCategory::RideOrder),
            Category::DailyChartCheck => Some(SeedCategory::DailyChartCheck),
            Category::Other => None,
        }
    }
}
