//! Translation shim.
//!
//! Every user-visible string in Rust code goes through [`t`], so none of them
//! are hardcoded at the call site even though no catalogue is wired up yet.
//! When gettext arrives, this function becomes the gettext call and nothing
//! else in the app changes. Strings in `.ui` files carry `translatable="yes"`
//! and are picked up by the same catalogue.

/// Mark and translate a user-visible string.
pub fn t(msgid: &str) -> String {
    msgid.to_string()
}
