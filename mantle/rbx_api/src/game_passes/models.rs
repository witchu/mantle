use serde::Deserialize;

use crate::models::AssetId;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GamePassConfigV2 {
    pub game_pass_id: AssetId,
    pub name: String,
    pub description: String,
    pub is_for_sale: bool,
    pub icon_asset_id: AssetId,
    pub price_information: Option<PriceInformation>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct PriceInformation {
    pub default_price_in_robux: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListGamePassesResponse {
    pub game_passes: Vec<GamePassConfigV2>,
    pub next_page_token: Option<String>,
}
