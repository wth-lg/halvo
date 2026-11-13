use super::Keychain;
use security_framework::base::Error;
use security_framework::item::{ItemClass, ItemSearchOptions, Limit, SearchResult};
use security_framework::os::macos::keychain::SecKeychain;

const PARAM: i32 = -50;
const INTERACTION_STILL_ALLOWED: i32 = -1;

pub(crate) struct SystemKeychain;

fn quiet<T>(call: impl FnOnce() -> Result<T, Error>) -> Result<T, i32> {
    let _quiet = SecKeychain::disable_user_interaction().map_err(|error| error.code())?;
    match SecKeychain::user_interaction_allowed() {
        Ok(false) => {}
        Ok(true) => return Err(INTERACTION_STILL_ALLOWED),
        Err(error) => return Err(error.code()),
    }
    call().map_err(|error| error.code())
}

fn query(service: &str, account: &str) -> ItemSearchOptions {
    let mut options = ItemSearchOptions::new();
    options
        .class(ItemClass::generic_password())
        .service(service)
        .account(account);
    options
}

impl Keychain for SystemKeychain {
    fn count(&self, service: &str, account: &str) -> Result<usize, i32> {
        quiet(|| {
            let mut options = query(service, account);
            options.load_attributes(true).limit(Limit::All);
            options.search().map(|found| found.len())
        })
    }

    fn data(&self, service: &str, account: &str) -> Result<Vec<u8>, i32> {
        let found = quiet(|| {
            let mut options = query(service, account);
            options.load_data(true).limit(1);
            options.search()
        })?;
        match found.into_iter().next() {
            Some(SearchResult::Data(bytes)) => Ok(bytes),
            _ => Err(PARAM),
        }
    }

    fn add(&self, service: &str, account: &str, bytes: &[u8]) -> Result<(), i32> {
        quiet(|| SecKeychain::default()?.add_generic_password(service, account, bytes))
    }

    fn delete(&self, service: &str, account: &str) -> Result<(), i32> {
        quiet(|| query(service, account).delete())
    }
}
