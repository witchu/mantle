use serde::Deserialize;

use crate::models::AssetId;

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetExperienceThumbnailResponse {
    pub id: AssetId,
    #[serde(default)]
    pub asset_type: Option<String>,
}

#[derive(Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct GetExperienceThumbnailsResponse {
    pub data: Vec<GetExperienceThumbnailResponse>,
}
