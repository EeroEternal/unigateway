//! Admin operations on `GatewayState`, split by concern. The module is
//! private; embedders reach these operations through the public
//! `GatewayState` methods unchanged.

mod api_keys;
mod mutations;
#[cfg(test)]
mod tests;
mod views;
