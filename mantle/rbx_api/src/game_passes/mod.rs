pub mod models;

use std::path::PathBuf;

use reqwest::multipart::Form;

use crate::{
    errors::RobloxApiResult,
    helpers::{get_file_part, handle, handle_as_json},
    models::AssetId,
    RobloxApi,
};

use self::models::{GamePassConfigV2, ListGamePassesResponse};

impl RobloxApi {
    pub async fn list_game_passes(
        &self,
        experience_id: AssetId,
        page_token: Option<String>,
    ) -> RobloxApiResult<ListGamePassesResponse> {
        let res = self
            .csrf_token_store
            .send_request(|| async {
                let mut req = self
                    .client
                    .get(format!(
                        "https://apis.roblox.com/game-passes/v1/universes/{}/game-passes/creator",
                        experience_id
                    ))
                    .query(&[("pageSize", 50.to_string())]);
                if let Some(page_token) = &page_token {
                    req = req.query(&[("pageToken", page_token)]);
                }
                Ok(req)
            })
            .await;

        handle_as_json(res).await
    }

    pub async fn get_all_game_passes(
        &self,
        experience_id: AssetId,
    ) -> RobloxApiResult<Vec<GamePassConfigV2>> {
        let mut all_game_passes = Vec::new();

        let mut page_token: Option<String> = None;
        loop {
            let res = self.list_game_passes(experience_id, page_token).await?;
            all_game_passes.extend(res.game_passes);

            if res
                .next_page_token
                .as_deref()
                .unwrap_or_default()
                .is_empty()
            {
                break;
            }

            page_token = res.next_page_token;
        }

        Ok(all_game_passes)
    }

    pub async fn create_game_pass(
        &self,
        experience_id: AssetId,
        name: String,
        price: Option<u32>,
        description: String,
        icon_file: PathBuf,
    ) -> RobloxApiResult<GamePassConfigV2> {
        let res = self
            .csrf_token_store
            .send_request(|| async {
                let mut form = Form::new()
                    .text("name", name.clone())
                    .text("description", description.clone())
                    .text("isForSale", price.is_some().to_string())
                    .part("imageFile", get_file_part(&icon_file).await?);
                if let Some(price) = &price {
                    form = form.text("price", price.to_string());
                }

                Ok(self
                    .client
                    .post(format!(
                        "https://apis.roblox.com/game-passes/v1/universes/{}/game-passes",
                        experience_id
                    ))
                    .multipart(form))
            })
            .await;

        handle_as_json(res).await
    }

    pub async fn update_game_pass(
        &self,
        experience_id: AssetId,
        game_pass_id: AssetId,
        name: String,
        description: String,
        price: Option<u32>,
        icon_file: Option<PathBuf>,
    ) -> RobloxApiResult<()> {
        let res = self
            .csrf_token_store
            .send_request(|| async {
                let mut form = Form::new()
                    .text("name", name.clone())
                    .text("description", description.clone())
                    .text("isForSale", price.is_some().to_string());
                if let Some(price) = &price {
                    form = form.text("price", price.to_string());
                }
                if let Some(icon_file) = &icon_file {
                    form = form.part("file", get_file_part(icon_file).await?);
                }

                Ok(self
                    .client
                    .patch(format!(
                        "https://apis.roblox.com/game-passes/v1/universes/{}/game-passes/{}",
                        experience_id, game_pass_id
                    ))
                    .multipart(form))
            })
            .await;

        handle(res).await?;

        Ok(())
    }
}
