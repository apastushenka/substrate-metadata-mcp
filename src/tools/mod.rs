//! The tools this server offers, a module each: the request and response types, whose
//! documentation becomes the tool's schema, and the function that answers. The tool's own
//! description sits on its `#[tool]` method in `server`.

mod pallet_item;
mod pallet_items;
mod pallets;
mod runtime_api_method;
mod runtime_api_methods;
mod runtime_apis;
mod runtime_info;
mod search;

pub use self::{
    pallet_item::{PalletItemAddress, PalletItemDetails, pallet_item},
    pallet_items::{PalletItems, PalletItemsRequest, pallet_items},
    pallets::{PalletList, pallets},
    runtime_api_method::{RuntimeApiMethodAddress, RuntimeApiMethodDetails, runtime_api_method},
    runtime_api_methods::{RuntimeApiMethods, RuntimeApiMethodsRequest, runtime_api_methods},
    runtime_apis::{RuntimeApiList, runtime_apis},
    runtime_info::{RuntimeInfo, runtime_info},
    search::{SearchRequest, SearchResults, search},
};
